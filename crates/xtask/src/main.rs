use std::{env, error::Error};

fn main() -> Result<(), Box<dyn Error>> {
	print_help();
	env::args().nth(1).map_or(Ok(()), |task| Err(format!("Unknown task: {task}").into()))
}

fn print_help() {
	println!("Tasks:");
	println!("	(none yet)");
}
