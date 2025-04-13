use battery::Manager;
use colored::Colorize;
use sysinfo::{ComponentExt, CpuExt, DiskExt, NetworkExt, ProcessExt, System, SystemExt};
use crossterm::{cursor, style::Stylize, terminal, ExecutableCommand};
use std::{fmt::format, io::{stdout, Write}, iter::Filter, os::linux::raw::stat, sync::Arc, thread, time::{Duration, Instant}};
use ctrlc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs::File;
use std::io::Write as ioWrite;

fn draw_bar_cpu(percentage: f32, width: usize) -> String {
   
    let filled = (percentage / 100.0 * width as f32).round() as usize;
    let empty=width.saturating_sub(filled);
    let color=match percentage {
        p if p>=60.0=>"█".repeat(filled).red(),
        p if p>=50.0=>"█".repeat(filled).yellow(),
        _=>"█".repeat(filled).green(),
        
    };
   format!("[{}{}]",color," ".repeat(empty))
}
fn draw_bar_battery(percentage: f32, width: usize) -> String {
    let filled = (percentage / 100.0 * width as f32).round() as usize;
    let empty=width.saturating_sub(filled);
    let color=match percentage {
        p if p>=60.0=>"█".repeat(filled).green(),
        p if p>=50.0=>"█".repeat(filled).yellow(),
        _=>"█".repeat(filled).red(),
        
    };
   format!("[{}{}]",color," ".repeat(empty))
}

fn display_battery_info(manager: &Manager) ->( f32,String) {
    let mut battery_percentage = 0.0;
    let mut battery_state= String::new();
    for battery in manager.batteries().unwrap() {
        let percentage = battery.as_ref().unwrap().state_of_charge();
        let state = battery.as_ref().unwrap().state();
        battery_percentage = percentage.into();
        battery_state=match state {
            battery::State::Charging => "Charging".to_string(),
            battery::State::Discharging => "Discharging".to_string(),
            battery::State::Full => "Full".to_string(),
            battery::State::Empty => "Empty".to_string(),
            battery::State::Unknown => "Unknown".to_string(),
            _=>"failed".to_string()
            
        };
    }
   ( battery_percentage,battery_state)
}

fn main() {
    let mut sys = System::new_all();
    let mut stdout = stdout();
    stdout.execute(terminal::Clear(terminal::ClearType::All)).unwrap();
    stdout.execute(cursor::Hide).unwrap();
    let mut last_battery_update = Instant::now(); 
    let manager = Manager::new().unwrap();
    let (mut battery_percentage,mut battery_state) =display_battery_info(&manager);
    let running = Arc::new(AtomicBool::new(true));
    let r=running.clone();
    use std::collections::HashMap;

    let mut prev_received: HashMap<String, u64> = HashMap::new();
    let mut prev_transmitted: HashMap<String, u64> = HashMap::new();

    ctrlc::set_handler(move||{
        r.store(false, Ordering::SeqCst);
        println!("Exiting and writing to file...");
    }).expect("Error setting Ctrl-C handler");
    let mut log=String::new();
    //Loop to display system information and to check for ctrlc
    while running.load(Ordering::SeqCst){
        let now=chrono::Local::now().format("%D:%H:%M:%S").to_string();
        sys.refresh_all();
        stdout.execute(terminal::Clear(terminal::ClearType::All)).unwrap();
        stdout.execute(cursor::MoveTo(100, 1)).unwrap();
        writeln!(stdout, "{}" , "Memory and CPU Usage".to_string().red()).unwrap();
        let cpu = sys.global_cpu_info();

        // Memory
        let used = sys.used_memory() as f64 / 1024.0 / 1024.0;
        let total = sys.total_memory() as f64 / 1024.0 / 1024.0;
        let ram_percent = (used / total) * 100.0;
        stdout.execute(cursor::MoveTo(100, 2)).unwrap();
        writeln!(stdout, "RAM Usage     : {:>6.2} / {:>6.2} GB", used, total).unwrap();
        log.push_str(&format!("[{}] RAM Usage: {:>6.2} / {:>6.2} GB\n",now, used, total));
        stdout.execute(cursor::MoveTo(135, 2)).unwrap();
        writeln!(stdout, "{}", draw_bar_cpu(ram_percent as f32, 20)).unwrap();
        stdout.execute(cursor::MoveTo(100, 3)).unwrap();
        writeln!(stdout, "CPU Usage     : {:>5.2}%", cpu.cpu_usage()).unwrap();
        stdout.execute(cursor::MoveTo(135, 3)).unwrap();
        writeln!(stdout, "{}", draw_bar_cpu(cpu.cpu_usage() as f32, 20)).unwrap();
        log.push_str(&format!("[{}] CPU usage:{:>5.2}%\n",now,cpu.cpu_usage()));

        for (i, cpu) in sys.cpus().iter().enumerate() {
            stdout.execute(cursor::MoveTo(100, i as u16 + 4)).unwrap();
            writeln!(stdout, "Core {:>2} : {:5.2}  ", i + 1, cpu.cpu_usage()).unwrap();
            stdout.execute(cursor::MoveTo(135, i as u16 + 4)).unwrap();
            writeln!(stdout, "{}", draw_bar_cpu(cpu.cpu_usage() as f32, 20)).unwrap();
        }
        stdout.execute(cursor::MoveTo(100,16)).unwrap();
        writeln!(stdout,"{}","Temperature".to_string().red()).unwrap();
        stdout.execute(cursor::MoveTo(100,17)).unwrap();
        for comp in sys.components(){
            writeln!(stdout,"{}: {}°C",comp.label(),comp.temperature()).unwrap();
        }
        stdout.execute(cursor::MoveTo(100, 19)).unwrap();
        writeln!(stdout,"{}","Load Average".to_string().red()).unwrap();
        let mut load=sys.load_average();
        stdout.execute(cursor::MoveTo(100, 20)).unwrap();
        writeln!(stdout,"1 min load: {} | 5 min load: {} | 15 min load: {}",load.one,load.five,load.fifteen).unwrap();

        // Disk
        stdout.execute(cursor::MoveTo(0, 1)).unwrap();
        writeln!(stdout, "{}","Disk Usage".to_string().red()).unwrap();
        stdout.execute(cursor::MoveTo(0, 2)).unwrap();
        for disk in sys.disks() {
            writeln!(stdout, "Disk: {}", disk.name().to_string_lossy()).unwrap();
            writeln!(stdout, "  Total       : {:>6.2} GB", disk.total_space() as f64 / 1_073_741_824.0).unwrap();
            writeln!(stdout, "  Available   : {:>6.2} GB", disk.available_space() as f64 / 1_073_741_824.0).unwrap();
        }


        // Network Interfaces
        stdout.execute(cursor::MoveTo(0, 8)).unwrap();
        writeln!(stdout, "{}","Network Interfaces".to_string().red()).unwrap();
        for (interface_name, data) in sys.networks() {
            let name = interface_name.clone();
            let received = data.total_received();
            let transmitted = data.total_transmitted();
        
            let prev_recv = prev_received.get(&name).cloned().unwrap_or(received);
            let prev_trans = prev_transmitted.get(&name).cloned().unwrap_or(transmitted);
        
            let download_speed = received.saturating_sub(prev_recv); 
            let upload_speed = transmitted.saturating_sub(prev_trans); 
        
            writeln!(
                stdout,
                "Interface: {:<10} | ↓ {:>6.2} KB/s | ↑ {:>6.2} KB/s",
                name,
                download_speed as f64 / 1024.0,
                upload_speed as f64 / 1024.0
            ).unwrap();
            prev_received.insert(name.clone(), received);
            prev_transmitted.insert(name.clone(), transmitted);
        }

        stdout.execute(cursor::MoveTo(0, 11)).unwrap();
        writeln!(stdout, "{}","System".to_string().red()).unwrap();
        writeln!(stdout, "Os:{}", sys.name().unwrap()).unwrap();
        let uptime=sys.uptime();
        let formatted = format!("{}h:{}m:{}s", uptime / 3600, (uptime % 3600) / 60, uptime % 60);
        writeln!(stdout, "uptime:{}", formatted).unwrap();
        writeln!(stdout, "Kernel:{}", sys.kernel_version().unwrap()).unwrap();
        writeln!(stdout, "Host:{}", sys.host_name().unwrap()).unwrap();

        stdout.execute(cursor::MoveTo(0, 16)).unwrap();
        writeln!(stdout, "{}","Processes".to_string().red()).unwrap();
        let mut processes: Vec<_> = sys.processes().iter().collect();
        processes.sort_by(|a, b| b.1.cpu_usage().partial_cmp(&a.1.cpu_usage()).unwrap());

        for process in processes.iter().take(20) {
            writeln!(stdout, "Process: {} | PID: {} | Memory: {}B | CPU: {}%", process.1.name(), process.0, process.1.memory(), process.1.cpu_usage()).unwrap();
        }

        // Battery Info to be updated every 400 seconds
        writeln!(stdout, "{}","Battery Info".to_string().red()).unwrap();
        if last_battery_update.elapsed() >= Duration::from_secs(400) {
            (battery_percentage,battery_state) = display_battery_info(&manager);
            last_battery_update = Instant::now();
        }
            let percentage =battery_percentage* 100.0;
            writeln!(stdout, "Battery Percentage: {:2}%", percentage).unwrap();
            writeln!(stdout, "Battery State: {}", battery_state).unwrap();
            writeln!(stdout, "{}", draw_bar_battery(percentage, 10)).unwrap();
        stdout.flush().unwrap();
        thread::sleep(Duration::from_secs(1));

    }
    let timestamp=chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let filename=format!("system_info_{}.txt",timestamp);
    let mut file=File::create(filename).unwrap();
    file.write_all(log.as_bytes()).unwrap();


 
}
