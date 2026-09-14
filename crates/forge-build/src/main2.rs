use forge_build::{execute, load_graph, Action, Driver};
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "usage: forge <build|check|run|test|graph> [--manifest-path PATH] [--target NAME] \
         [--driver PROGRAM] [--driver-arg ARG]... [-- PROGRAM_ARGS...]"
    );
    std::process::exit(64);
}

fn normalize_package_module(name: &str) -> String {
    name.replace('-', "_")
}

fn library_driver_args(
    graph: &forge_build::BuildGraph,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut args = Vec::new();
    for package_name in &graph.order {
        if package_name == &graph.root {
            continue;
        }
        let package = &graph.packages[package_name];
        let libraries: Vec<_> = package
            .targets
            .values()
            .filter(|target| target.kind == "library")
            .collect();
        if libraries.len() != 1 {
            return Err(format!(
                "dependency package {} must expose exactly one :library target; found {}",
                package.name,
                libraries.len()
            )
            .into());
        }
        let library = libraries[0];
        args.push("--library".to_owned());
        args.push(format!(
            "{}={}",
            normalize_package_module(&package.name),
            library.root.display()
        ));
    }
    Ok(args)
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
    let mut program_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                program_args.extend(args);
                break;
            }
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
        if !program_args.is_empty() {
            usage();
        }
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

    if !program_args.is_empty() && action != Some(Action::Run) {
        return Err("program arguments are only valid with forge run".into());
    }

    driver_args.extend(library_driver_args(&graph)?);
    for arg in program_args {
        driver_args.push("--program-arg".to_owned());
        driver_args.push(arg);
    }

    let driver = Driver {
        program: driver_program,
        prefix_args: driver_args,
    };
    execute(&graph, &driver, action.unwrap(), target.as_deref())?;
    Ok(())
}
