use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    Keyword(String),
    Bool(bool),
    Map(BTreeMap<String, Value>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub kind: String,
    pub root: PathBuf,
    pub std: bool,
    pub entry: Option<String>,
    pub expected_output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub manifest_path: PathBuf,
    pub root_dir: PathBuf,
    pub name: String,
    pub version: String,
    pub dependencies: BTreeMap<String, Dependency>,
    pub targets: BTreeMap<String, Target>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildGraph {
    pub root: String,
    pub packages: BTreeMap<String, Package>,
    pub order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driver {
    pub program: String,
    pub prefix_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Build,
    Check,
    Run,
    Test,
}

#[derive(Debug)]
pub struct BuildError(pub String);

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BuildError {}

type Result<T> = std::result::Result<T, BuildError>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    LBrace,
    RBrace,
    Keyword(String),
    String(String),
    Bool(bool),
    Tag(String),
}

fn lex(input: &str) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            c if c.is_whitespace() || c == ',' => i += 1,
            ';' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '{' => {
                out.push(Token::LBrace);
                i += 1;
            }
            '}' => {
                out.push(Token::RBrace);
                i += 1;
            }
            ':' => {
                i += 1;
                let start = i;
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], '{' | '}' | ',')
                {
                    i += 1;
                }
                out.push(Token::Keyword(chars[start..i].iter().collect()));
            }
            '#' => {
                i += 1;
                let start = i;
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], '{' | '}' | ',')
                {
                    i += 1;
                }
                out.push(Token::Tag(chars[start..i].iter().collect()));
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < chars.len() {
                    match chars[i] {
                        '"' => {
                            i += 1;
                            closed = true;
                            break;
                        }
                        '\\' => {
                            i += 1;
                            if i >= chars.len() {
                                break;
                            }
                            let c = match chars[i] {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                '"' => '"',
                                '\\' => '\\',
                                other => {
                                    return Err(BuildError(format!(
                                        "unsupported string escape \\{other}"
                                    )))
                                }
                            };
                            s.push(c);
                            i += 1;
                        }
                        c => {
                            s.push(c);
                            i += 1;
                        }
                    }
                }
                if !closed {
                    return Err(BuildError("unterminated string in forge.fdn".into()));
                }
                out.push(Token::String(s));
            }
            _ => {
                let start = i;
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], '{' | '}' | ',')
                {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                match word.as_str() {
                    "true" => out.push(Token::Bool(true)),
                    "false" => out.push(Token::Bool(false)),
                    _ => return Err(BuildError(format!("unexpected token {word}"))),
                }
            }
        }
    }
    Ok(out)
}

fn parse_value(tokens: &[Token], pos: &mut usize) -> Result<Value> {
    let token = tokens
        .get(*pos)
        .ok_or_else(|| BuildError("unexpected end of forge.fdn".into()))?;
    match token {
        Token::String(s) => {
            *pos += 1;
            Ok(Value::String(s.clone()))
        }
        Token::Keyword(s) => {
            *pos += 1;
            Ok(Value::Keyword(s.clone()))
        }
        Token::Bool(v) => {
            *pos += 1;
            Ok(Value::Bool(*v))
        }
        Token::LBrace => parse_map(tokens, pos).map(Value::Map),
        other => Err(BuildError(format!("unexpected value token {other:?}"))),
    }
}

fn parse_map(tokens: &[Token], pos: &mut usize) -> Result<BTreeMap<String, Value>> {
    if tokens.get(*pos) != Some(&Token::LBrace) {
        return Err(BuildError("expected {".into()));
    }
    *pos += 1;
    let mut map = BTreeMap::new();
    while tokens.get(*pos) != Some(&Token::RBrace) {
        let key = match tokens.get(*pos) {
            Some(Token::Keyword(k)) => k.clone(),
            other => return Err(BuildError(format!("expected keyword map key, found {other:?}"))),
        };
        *pos += 1;
        let value = parse_value(tokens, pos)?;
        if map.insert(key.clone(), value).is_some() {
            return Err(BuildError(format!("duplicate manifest key :{key}")));
        }
    }
    *pos += 1;
    Ok(map)
}

pub fn parse_manifest_text(input: &str) -> Result<BTreeMap<String, Value>> {
    let tokens = lex(input)?;
    let mut pos = 0;
    match tokens.get(pos) {
        Some(Token::Tag(tag)) if tag == "forge/package" => pos += 1,
        Some(Token::Tag(tag)) => {
            return Err(BuildError(format!("expected #forge/package, found #{tag}")))
        }
        _ => return Err(BuildError("manifest must begin with #forge/package".into())),
    }
    let map = parse_map(&tokens, &mut pos)?;
    if pos != tokens.len() {
        return Err(BuildError("trailing tokens after package manifest".into()));
    }
    Ok(map)
}

fn as_map<'a>(value: &'a Value, what: &str) -> Result<&'a BTreeMap<String, Value>> {
    match value {
        Value::Map(v) => Ok(v),
        _ => Err(BuildError(format!("{what} must be a map"))),
    }
}

fn required_string(map: &BTreeMap<String, Value>, key: &str) -> Result<String> {
    match map.get(key) {
        Some(Value::String(v)) => Ok(v.clone()),
        _ => Err(BuildError(format!("missing or invalid :{key}"))),
    }
}

fn optional_string(map: &BTreeMap<String, Value>, key: &str) -> Result<Option<String>> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::String(v)) => Ok(Some(v.clone())),
        _ => Err(BuildError(format!(":{key} must be a string"))),
    }
}

fn optional_bool(map: &BTreeMap<String, Value>, key: &str, default: bool) -> Result<bool> {
    match map.get(key) {
        None => Ok(default),
        Some(Value::Bool(v)) => Ok(*v),
        _ => Err(BuildError(format!(":{key} must be boolean"))),
    }
}

fn required_keyword(map: &BTreeMap<String, Value>, key: &str) -> Result<String> {
    match map.get(key) {
        Some(Value::Keyword(v)) => Ok(v.clone()),
        _ => Err(BuildError(format!("missing or invalid :{key}"))),
    }
}

pub fn load_package(manifest_path: &Path) -> Result<Package> {
    let manifest_path = fs::canonicalize(manifest_path).map_err(|e| {
        BuildError(format!("cannot open manifest {}: {e}", manifest_path.display()))
    })?;
    let root_dir = manifest_path
        .parent()
        .ok_or_else(|| BuildError("manifest has no parent directory".into()))?
        .to_path_buf();
    let source = fs::read_to_string(&manifest_path)
        .map_err(|e| BuildError(format!("cannot read {}: {e}", manifest_path.display())))?;
    let map = parse_manifest_text(&source)?;
    let name = required_string(&map, "name")?;
    let version = required_string(&map, "version")?;

    let mut dependencies = BTreeMap::new();
    if let Some(value) = map.get("dependencies") {
        for (dep_name, dep_value) in as_map(value, ":dependencies")? {
            let dep_map = as_map(dep_value, "dependency")?;
            let relative = required_string(dep_map, "path")?;
            let path = root_dir.join(relative).join("forge.fdn");
            dependencies.insert(
                dep_name.clone(),
                Dependency {
                    name: dep_name.clone(),
                    path,
                },
            );
        }
    }

    let target_map = as_map(
        map.get("targets")
            .ok_or_else(|| BuildError("missing :targets".into()))?,
        ":targets",
    )?;
    let mut targets = BTreeMap::new();
    for (target_name, value) in target_map {
        let m = as_map(value, "target")?;
        let kind = required_keyword(m, "kind")?;
        if !matches!(kind.as_str(), "library" | "executable" | "kernel" | "test") {
            return Err(BuildError(format!("unsupported target kind :{kind}")));
        }
        let root = root_dir.join(required_string(m, "root")?);
        if !root.is_file() {
            return Err(BuildError(format!("target root does not exist: {}", root.display())));
        }
        let std_default = kind != "kernel";
        let expected_output = if let Some(test) = m.get("test") {
            let test = as_map(test, ":test")?;
            optional_string(test, "expected")?.map(|p| root_dir.join(p))
        } else {
            None
        };
        targets.insert(
            target_name.clone(),
            Target {
                name: target_name.clone(),
                kind,
                root,
                std: optional_bool(m, "std", std_default)?,
                entry: optional_string(m, "entry")?,
                expected_output,
            },
        );
    }

    Ok(Package {
        manifest_path,
        root_dir,
        name,
        version,
        dependencies,
        targets,
    })
}

pub fn load_graph(manifest_path: &Path) -> Result<BuildGraph> {
    let root_package = load_package(manifest_path)?;
    let root = root_package.name.clone();
    let mut packages = BTreeMap::new();
    let mut order = Vec::new();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();

    fn visit(
        package: Package,
        packages: &mut BTreeMap<String, Package>,
        order: &mut Vec<String>,
        visiting: &mut BTreeSet<PathBuf>,
        visited: &mut BTreeSet<PathBuf>,
    ) -> Result<()> {
        let path = package.manifest_path.clone();
        if visited.contains(&path) {
            return Ok(());
        }
        if !visiting.insert(path.clone()) {
            return Err(BuildError(format!("dependency cycle at {}", path.display())));
        }
        for dep in package.dependencies.values() {
            let child = load_package(&dep.path)?;
            if child.name != dep.name {
                return Err(BuildError(format!(
                    "dependency :{} points to package {}",
                    dep.name, child.name
                )));
            }
            visit(child, packages, order, visiting, visited)?;
        }
        visiting.remove(&path);
        visited.insert(path);
        if let Some(existing) = packages.get(&package.name) {
            if existing.manifest_path != package.manifest_path {
                return Err(BuildError(format!(
                    "package name {} resolves to multiple local paths",
                    package.name
                )));
            }
        } else {
            order.push(package.name.clone());
            packages.insert(package.name.clone(), package);
        }
        Ok(())
    }

    visit(
        root_package,
        &mut packages,
        &mut order,
        &mut visiting,
        &mut visited,
    )?;
    Ok(BuildGraph { root, packages, order })
}

fn selected_targets<'a>(package: &'a Package, name: Option<&str>) -> Result<Vec<&'a Target>> {
    if let Some(name) = name {
        return package
            .targets
            .get(name)
            .map(|t| vec![t])
            .ok_or_else(|| BuildError(format!("unknown target :{name}")));
    }
    Ok(package.targets.values().collect())
}

fn invoke(driver: &Driver, mode: &str, target: &Target) -> Result<Output> {
    let mut command = Command::new(&driver.program);
    command.args(&driver.prefix_args);
    command.arg(mode);
    command.arg(&target.root);
    command.output().map_err(|e| {
        BuildError(format!(
            "failed to execute driver {} for target {}: {e}",
            driver.program, target.name
        ))
    })
}

fn require_success(output: &Output, target: &Target) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(BuildError(format!(
            "target {} failed\nstdout:\n{}\nstderr:\n{}",
            target.name,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

pub fn execute(
    graph: &BuildGraph,
    driver: &Driver,
    action: Action,
    target_name: Option<&str>,
) -> Result<()> {
    let package = graph
        .packages
        .get(&graph.root)
        .ok_or_else(|| BuildError("root package missing from graph".into()))?;
    let targets = selected_targets(package, target_name)?;

    for target in targets {
        match action {
            Action::Build | Action::Check => {
                let output = invoke(driver, "--check", target)?;
                require_success(&output, target)?;
            }
            Action::Run => {
                if target.kind == "library" {
                    continue;
                }
                let output = invoke(driver, "--run", target)?;
                require_success(&output, target)?;
                print!("{}", String::from_utf8_lossy(&output.stdout));
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
            }
            Action::Test => {
                let output = invoke(driver, "--run", target)?;
                require_success(&output, target)?;
                if let Some(expected_path) = &target.expected_output {
                    let expected = fs::read(expected_path).map_err(|e| {
                        BuildError(format!("cannot read {}: {e}", expected_path.display()))
                    })?;
                    if output.stdout != expected {
                        return Err(BuildError(format!(
                            "target {} output differs from {}",
                            target.name,
                            expected_path.display()
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("forge-build-{name}-{n}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn parses_minimal_manifest() {
        let text = r#"#forge/package {
          :name "demo"
          :version "0.1.0"
          :targets { :main { :kind :executable :root "src/main.fg" } }
          :dependencies {}
        }"#;
        let value = parse_manifest_text(text).unwrap();
        assert_eq!(required_string(&value, "name").unwrap(), "demo");
    }

    #[test]
    fn resolves_local_dependencies_in_dependency_order() {
        let root = temp_dir("graph");
        let dep = root.join("dep");
        let app = root.join("app");
        fs::create_dir_all(dep.join("src")).unwrap();
        fs::create_dir_all(app.join("src")).unwrap();
        fs::write(dep.join("src/lib.fg"), "module dep;\n").unwrap();
        fs::write(app.join("src/main.fg"), "module app;\n").unwrap();
        fs::write(
            dep.join("forge.fdn"),
            "#forge/package { :name \"dep\" :version \"0.1.0\" :targets { :lib { :kind :library :root \"src/lib.fg\" } } :dependencies {} }",
        )
        .unwrap();
        fs::write(
            app.join("forge.fdn"),
            "#forge/package { :name \"app\" :version \"0.1.0\" :targets { :main { :kind :executable :root \"src/main.fg\" } } :dependencies { :dep { :path \"../dep\" } } }",
        )
        .unwrap();
        let graph = load_graph(&app.join("forge.fdn")).unwrap();
        assert_eq!(graph.order, vec!["dep", "app"]);
    }

    #[test]
    fn kernel_defaults_to_no_std() {
        let root = temp_dir("kernel");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.fg"), "module kernel;\n").unwrap();
        fs::write(
            root.join("forge.fdn"),
            "#forge/package { :name \"kernel\" :version \"0.1.0\" :targets { :kernel { :kind :kernel :root \"src/main.fg\" :entry \"start\" } } }",
        )
        .unwrap();
        let package = load_package(&root.join("forge.fdn")).unwrap();
        assert!(!package.targets["kernel"].std);
        assert_eq!(package.targets["kernel"].entry.as_deref(), Some("start"));
    }
}
