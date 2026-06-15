use astro_recover::{decode_json, force_land};
use std::fs;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage:\n  cli decode <save>\n  cli land <save> <ship_name> <pad_name> <out>");
        std::process::exit(2);
    }

    match args[1].as_str() {
        "decode" => {
            let data = fs::read(&args[2]).unwrap();
            println!("{}", decode_json(&data));
        }
        "land" => {
            let data = fs::read(&args[2]).unwrap();
            match force_land(&data, &args[3], &args[4]) {
                Ok(out) => {
                    fs::write(&args[5], &out).unwrap();
                    eprintln!("wrote {} ({} bytes)", args[5], out.len());
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("unknown cmd");
            std::process::exit(2);
        }
    }
}
