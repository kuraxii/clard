use std::fs;

fn main() {
    let json = fs::read_to_string("groups.json").unwrap_or_else(|_| "{}".to_string());
    println!("File loaded");
}
