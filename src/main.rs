use clap::{App, Arg};

mod batch;
mod interactive;

pub fn main() {
    let args = App::new("bencedit")
        .about("Bencode editor")
        .arg(Arg::with_name("batch")
             .help("Process several files through transforms")
             .long("batch")
             .short("b"))
        .arg(Arg::with_name("transform")
             .help("An action to apply to files in batch mode")
             .requires("batch")
             .takes_value(true)
             .number_of_values(1)
             .multiple(true)
             .long("transform")
             .short("t"))
        .arg(Arg::with_name("skip_invalid")
             .help("In batch mode, skip invalid files")
             .requires("batch")
             .long("skip-invalid")
             .short("S"))
        .arg(Arg::with_name("skip_not_found")
             .help("In batch mode, skip non-existant files")
             .requires("batch")
             .long("skip-not-found")
             .short("N"))
        .arg(Arg::with_name("files")
             .multiple(true)
             .required(true))
        .get_matches();

    if args.is_present("batch") {
        if let Err(e) = batch::batch(args.values_of("files").unwrap().collect()) {
            eprintln!("Error: {}", e);
        }
    } else {
        if args.occurrences_of("files") > 1 {
            println!("Warning: Many files were passed to interactive mode, only the first one will be loaded.")
        }

        if let Err(e) = interactive::interactive(args.value_of("files").unwrap()) {
            eprintln!("Error: {}", e);
        }
    }
}
