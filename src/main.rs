mod core;
mod files;
pub mod remote;
mod tester;
mod views;

use core::add::add;
use core::branch::branch;
use core::clone::clone;
use core::commit::commit;
use core::init::init;
use core::merge::merge;
use core::pull::pull;
use core::push::push;
use core::remote::remote;
use core::view::view;
use std::env;
use tester::generate::generate;

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
        "commit" => commit(&args),
        "branch" => branch(&args),
        "remote" => remote(&args),
        "view" => view(&args),
        "clone" => clone(&args),
        "push" => push(&args),
        "pull" => pull(&args),
        "merge" => merge(&args),
        "generate" => generate(&args),
        _ => println!("Unknown command: {}", command),
    }
}
