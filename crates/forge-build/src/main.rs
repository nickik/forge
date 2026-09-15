use forge_build::{execute, load_graph, Action, Driver};
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "usage: forge <build|check|run|test|graph> [--manifest-path PATH] [--target NAME] \\\n         [--driver PROGRAM] [--driver-arg ARG]..."
    );
    std::process::exit(64);
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("forge: {error}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let action_text = args.next().unwrap_or_else(|| usage());
    let action = match action_text.as_str() {
        "build" => Some(Action::Build),
        "check" => Some(Action::Check),
        "run" => Some(Action::Run),
        "test" => Some(Action::Test),
        "graph" => None,
        _ => usage(),
    };

    let mut manifest = PathBuf::from("forge.fdn");
    let mut target = None;
    let mut driver_program = std::env::var("FORGE_DRIVER").unwrap_or_else(|_| "forgec".into());
    let mut driver_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest-path" => {
                manifest = PathBuf::from(args.next().unwrap_or_else(|| usage()));
            }
            "--target" => target = Some(args.next().unwrap_or_else(|| usage())),
            "--driver" => driver_program = args.next().unwrap_or_else(|| usage()),
            "--driver-arg" => driver_args.push(args.next().unwrap_or_else(|| usage())),
            _ => usage(),
        }
    }

    let graph = load_graph(&manifest)?;
    if action.is_none() {
        for name in &graph.order {
            let package = &graph.packages[name];
            println!(
                "{} {} {}",
                package.name,
                package.version,
                package.manifest_path.display()
            );
        }
        return Ok(());
    }

    let driver = Driver {
        program: driver_program,
        prefix_args: driver_args,
    };
    execute(&graph, &driver, action.unwrap(), target.as_deref())?;
    Ok(())
}
