use forge_build::{execute, load_graph, Action, Driver};
use std::fs;
use std::path::{Path, PathBuf};

fn usage() -> ! {
    eprintln!(
        "usage: forge <build|check|run|test|graph> [--manifest-path PATH] [--target NAME] \
         [--platform NAME] [--driver PROGRAM] [--driver-arg ARG]... [-- PROGRAM_ARGS...]"
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

fn toolchain_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let configured = std::env::var_os("FORGE_HOME").map(PathBuf::from);
    let root = configured.unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .to_path_buf()
    });
    Ok(fs::canonicalize(&root).map_err(|error| {
        format!(
            "cannot locate Forge toolchain root {}: {error}; set FORGE_HOME when running an installed toolchain",
            root.display()
        )
    })?)
}

fn library_arg(name: &str, path: &Path) -> Vec<String> {
    vec!["--library".to_owned(), format!("{name}={}", path.display())]
}

fn core_driver_args(toolchain: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let path = toolchain.join("lib/core.fg");
    if !path.is_file() {
        return Err(format!("missing Forge core library {}", path.display()).into());
    }
    Ok(library_arg("core", &path))
}

fn collect_forge_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_forge_files(&path, files)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("fg") {
            files.push(path);
        }
    }
    Ok(())
}

fn hosted_driver_args(toolchain: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let lib = toolchain.join("lib");
    let std_root = lib.join("std.fg");
    let std_dir = lib.join("std");
    if !std_root.is_file() || !std_dir.is_dir() {
        return Err(format!(
            "Forge hosted std library is incomplete under {}",
            lib.display()
        )
        .into());
    }

    let mut sources = vec![std_root.clone()];
    collect_forge_files(&std_dir, &mut sources)?;
    sources.sort();

    let mut args = Vec::new();
    for source in sources {
        let name = if source == std_root {
            "std".to_owned()
        } else {
            let relative = source.strip_prefix(&lib)?;
            let mut segments = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let last = segments
                .last_mut()
                .ok_or_else(|| format!("invalid std path {}", source.display()))?;
            *last = last
                .strip_suffix(".fg")
                .ok_or_else(|| format!("invalid Forge library path {}", source.display()))?
                .to_owned();
            segments.join(".")
        };
        args.extend(library_arg(&name, &source));
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
    let mut platform = None;
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
            "--platform" => platform = Some(args.next().unwrap_or_else(|| usage())),
            "--driver" => driver_program = args.next().unwrap_or_else(|| usage()),
            "--driver-arg" => driver_args.push(args.next().unwrap_or_else(|| usage())),
            _ => usage(),
        }
    }

    let graph = load_graph(&manifest)?;
    if action.is_none() {
        if !program_args.is_empty() || platform.is_some() {
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

    let toolchain = toolchain_root()?;
    driver_args.extend(core_driver_args(&toolchain)?);
    driver_args.extend(library_driver_args(&graph)?);
    if let Some(platform) = platform {
        driver_args.push("--platform".to_owned());
        driver_args.push(platform);
    }
    for arg in program_args {
        driver_args.push("--program-arg".to_owned());
        driver_args.push(arg);
    }

    let driver = Driver {
        program: driver_program,
        prefix_args: driver_args,
        hosted_args: hosted_driver_args(&toolchain)?,
    };
    execute(&graph, &driver, action.unwrap(), target.as_deref())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("forge-cli-{name}-{unique}"));
        fs::create_dir_all(&path).expect("create temporary package graph");
        path
    }

    #[test]
    fn transitive_dependency_libraries_follow_graph_order() {
        let root = temp_dir("dependency-driver-order");
        let leaf = root.join("leaf");
        let middle = root.join("middle");
        let app = root.join("app");
        for package in [&leaf, &middle, &app] {
            fs::create_dir_all(package.join("src"))
                .expect("create package source directory");
        }

        fs::write(leaf.join("src/lib.fg"), "module leaf_lib;\n")
            .expect("write leaf library");
        fs::write(
            leaf.join("forge.fdn"),
            r#"#forge/package {
              :name "leaf-lib"
              :version "0.1.0"
              :targets { :lib { :kind :library :root "src/lib.fg" } }
              :dependencies {}
            }"#,
        )
        .expect("write leaf manifest");

        fs::write(middle.join("src/lib.fg"), "module middle_lib;\n")
            .expect("write middle library");
        fs::write(
            middle.join("forge.fdn"),
            r#"#forge/package {
              :name "middle-lib"
              :version "0.1.0"
              :targets { :lib { :kind :library :root "src/lib.fg" } }
              :dependencies { :leaf-lib { :path "../leaf" } }
            }"#,
        )
        .expect("write middle manifest");

        fs::write(app.join("src/main.fg"), "module app;\n")
            .expect("write application source");
        fs::write(
            app.join("forge.fdn"),
            r#"#forge/package {
              :name "app"
              :version "0.1.0"
              :targets { :main { :kind :executable :root "src/main.fg" } }
              :dependencies { :middle-lib { :path "../middle" } }
            }"#,
        )
        .expect("write application manifest");

        let graph = load_graph(&app.join("forge.fdn")).expect("load transitive package graph");
        assert_eq!(
            graph.order,
            vec![
                "leaf-lib".to_owned(),
                "middle-lib".to_owned(),
                "app".to_owned(),
            ]
        );

        let leaf_root = fs::canonicalize(&leaf)
            .expect("canonical leaf package")
            .join("src/lib.fg");
        let middle_root = fs::canonicalize(&middle)
            .expect("canonical middle package")
            .join("src/lib.fg");
        assert_eq!(
            library_driver_args(&graph).expect("construct dependency driver arguments"),
            vec![
                "--library".to_owned(),
                format!("leaf_lib={}", leaf_root.display()),
                "--library".to_owned(),
                format!("middle_lib={}", middle_root.display()),
            ]
        );

        fs::remove_dir_all(root).expect("remove temporary package graph");
    }
}
