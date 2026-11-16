use std::{env, process::ExitCode};

mod release;

fn main() -> ExitCode {
	let result = if env::args().nth(1).as_deref() == Some("release") {
		release::release()
	} else {
		print_help();
		Ok(())
	};
	// Printed here rather than returned from main, which would print it with Debug, in quotes.
	if let Err(error) = result {
		eprintln!("Error: {error}");
		return ExitCode::FAILURE;
	}
	ExitCode::SUCCESS
}

fn print_help() {
	println!("Tasks:");
	println!("	release    Build Gitview in release mode, then package it as a zip and, on Windows, an installer");
}
