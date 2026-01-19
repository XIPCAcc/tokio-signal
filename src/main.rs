#![warn(clippy::all)]

use clap::{Parser, ValueEnum};
use nix::sys::signal::{kill, Signal as NixSignal, sigaction, SaFlags, SigAction, SigHandler, SigSet, sigprocmask, SigmaskHow};
use nix::unistd::Pid;
use std::error::Error;
use std::time::{Duration, Instant};
use tokio::signal::unix::{signal, SignalKind};
use libc;

/// 空的信号处理函数
extern "C" fn empty_signal_handler(_: libc::c_int) {
}

/// 信号屏蔽标志位
const BLOCK_USR1: i32 = 0x1;
const BLOCK_USR2: i32 = 0x2;

/// 设置需要忽略的信号（注册空处理函数）
fn setup_ignored_signals(flags: i32) -> Result<(), Box<dyn Error>> {
    // 创建信号动作结构体
    let signal_action = SigAction::new(
        SigHandler::Handler(empty_signal_handler), // 空的信号处理函数
        SaFlags::SA_RESTART,                       // 让系统调用在被信号中断后重启
        SigSet::empty(),                           // 初始化为空的信号集合
    );

    // Ignore SIGUSR1 ?
    if (flags & BLOCK_USR1) == 0 {
        // Set signal handler
        unsafe {
            sigaction(NixSignal::SIGUSR1, &signal_action)?;
        }
    }

    // Ignore SIGUSR2 ?
    if (flags & BLOCK_USR2) == 0 {
        // Set signal handler
        unsafe {
            sigaction(NixSignal::SIGUSR2, &signal_action)?;
        }
    }

    Ok(())
}

/// 设置需要屏蔽的信号
fn setup_blocked_signals(flags: i32) -> Result<(), Box<dyn Error>> {
    // 创建信号集合
    let mut mask = SigSet::empty();

    // Block SIGUSR1
    if (flags & BLOCK_USR1) != 0 {
        mask.add(NixSignal::SIGUSR1);
    }

    // Block SIGUSR2
    if (flags & BLOCK_USR2) != 0 {
        mask.add(NixSignal::SIGUSR2);
    }

    // Change signal mask
    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?;

    Ok(())
}

/// 设置信号处理和屏蔽
fn setup_signals(flags: i32) -> Result<(), Box<dyn Error>> {
    // 设置需要忽略的信号
    setup_ignored_signals(flags)?;

    // 设置需要屏蔽的信号
    setup_blocked_signals(flags)?;

    Ok(())
}

/// 设置服务器端信号屏蔽：屏蔽SIGUSR1，忽略SIGUSR2
fn setup_server_signals() -> Result<(), Box<dyn Error>> {
    setup_signals(BLOCK_USR1)?;
    std::thread::sleep(std::time::Duration::from_micros(1000));
    Ok(())
}

/// 设置客户端信号屏蔽：忽略SIGUSR1，屏蔽SIGUSR2
fn setup_client_signals() -> Result<(), Box<dyn Error>> {
    setup_signals(BLOCK_USR2)?;
    std::thread::sleep(std::time::Duration::from_micros(1000));
    Ok(())
}

/// 基准测试结构体，用于存储性能统计数据
#[derive(Debug)]
struct Benchmarks {
    total_start: Instant,
    single_start: Instant,
    minimum: Duration,
    maximum: Duration,
    sum: Duration,
    squared_sum: f64,
    count: usize,
}

impl Benchmarks {
    /// 创建新的基准测试结构体
    fn new() -> Self {
        Self {
            total_start: Instant::now(),
            single_start: Instant::now(),
            minimum: Duration::from_secs(u64::MAX),
            maximum: Duration::from_nanos(0),
            sum: Duration::from_nanos(0),
            squared_sum: 0.0,
            count: 0,
        }
    }
    
    /// 更新基准测试数据
    fn update(&mut self, duration: Duration) {
        self.minimum = self.minimum.min(duration);
        self.maximum = self.maximum.max(duration);
        self.sum += duration;
        self.squared_sum += duration.as_nanos() as f64 * duration.as_nanos() as f64;
        self.count += 1;
    }
    
    /// 评估基准测试结果
    fn evaluate(&self, args: &Args) {
        let total_time = self.total_start.elapsed();
        let average = self.sum / (self.count as u32);
        
        let sigma = self.squared_sum / self.count as f64;
        let sigma = (sigma - (average.as_nanos() as f64).powi(2)).sqrt();
        
        let message_rate = (self.count as f64) / total_time.as_secs_f64();
        let message_rate_mb = (self.count as f64 * args.size as f64) / 1024.0 / 1024.0 / total_time.as_secs_f64();
        
        println!("\n============ RESULTS ================");
        println!("Message size:       {}", args.size);
        println!("Message count:      {}", args.count);
        println!("Total duration:     {:.3} ms", total_time.as_millis() as f64);
        println!("Average duration:   {:.3} us", average.as_micros() as f64);
        println!("Minimum duration:   {:.3} us", self.minimum.as_micros() as f64);
        println!("Maximum duration:   {:.3} us", self.maximum.as_micros() as f64);
        println!("Standard deviation: {:.3} us", sigma / 1000.0);
        println!("Message rate:       {:.0} msg/s", message_rate);
        println!("Message rate:       {:.3} MB/s", message_rate_mb);
        println!("=====================================");
    }
}

/// 发送信号到目标 PID
fn send_signal(pid: u32, signal: NixSignal) -> Result<(), Box<dyn Error>> {
    let nix_pid = Pid::from_raw(pid as i32);
    kill(nix_pid, signal)?;
    Ok(())
}

#[derive(Parser, Debug)]
struct Args {
    /// ping-pong 次数
    #[arg(long, short, default_value_t = 1_000_000)]
    count: usize,

    #[arg(long, short, default_value_t = 1)]
    size: usize,

    /// 运行模式：server / client / test
    #[arg(long, short, value_enum, default_value_t = Mode::Server)]
    mode: Mode,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq)]
enum Mode {
    Server,
    Client,
}

async fn run_server(args: &Args) -> Result<(), Box<dyn Error>> {
    setup_server_signals()?;

    // 创建信号接收器
    let mut sigusr1 = signal(SignalKind::user_defined1())?;

    // 等待初始信号
    eprintln!("[SERVER] Waiting for initial signal from client...");
    sigusr1.recv().await;
    eprintln!("[SERVER] Received initial signal from client!");

    // 设置基准测试
    let mut bench = Benchmarks::new();

    for message in 0..args.count {
        bench.single_start = Instant::now();

        // eprintln!("[SERVER] Sending SIGUSR2 to client (message: {})..", message + 1);
        let _ = send_signal(0, NixSignal::SIGUSR2);

        // 等待响应信号
        // eprintln!("[SERVER] Waiting for SIGUSR1 from client (message: {})..", message + 1);
        sigusr1.recv().await;
        // eprintln!("[SERVER] Received SIGUSR1 from client (message: {})", message + 1);
        
        let total_duration = bench.single_start.elapsed();

        bench.update(total_duration);
    }

    bench.evaluate(args);
    Ok(())
}

async fn run_client(args: &Args) -> Result<(), Box<dyn Error>> {
    setup_client_signals()?;
    
    // 创建信号接收器
    let mut sigusr2 = signal(SignalKind::user_defined2())?;
    
    // 向进程组发送信号（使用PID 0）
    eprintln!("[CLIENT] Sending initial SIGUSR1 to server...");
    let _send_result = send_signal(0, NixSignal::SIGUSR1);
    eprintln!("[CLIENT] Sent initial SIGUSR1 to server!");
    
    let mut remaining = args.count;
    
    while remaining > 0 {
        // 等待来自服务器的信号
        // eprintln!("[CLIENT] Waiting for SIGUSR2 from server (remaining: {})..", remaining);
        sigusr2.recv().await;
        // eprintln!("[CLIENT] Received SIGUSR2 from server (remaining: {})", remaining);

        // 向进程组发送信号（使用PID 0）
        // eprintln!("[CLIENT] Sending SIGUSR1 to server (remaining: {})..", remaining);
        let _send_result = send_signal(0, NixSignal::SIGUSR1);
        // eprintln!("[CLIENT] Sent SIGUSR1 to server (remaining: {})", remaining - 1);
        
        remaining -= 1;
    }
    
    Ok(())
}

/// 异步主函数
async fn main_async() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    
    match args.mode {
        Mode::Server => run_server(&args).await,
        Mode::Client => run_client(&args).await,
    }
}

/// 主函数
fn main() -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(main_async())
}