mod core;
use core::add::add;
use core::init::init;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        println!("No commands provided");
        return;
    }

    let command = &args[1];

    match command.as_str() {
        "init" => init(&args),
        "add" => add(&args),
        _ => println!("Unknown command: {}", command),
    }
}
