use railweave_adapters::{detect_all, import_external, import_path};
use railweave_compose::compose_manifest;
use railweave_core::{Detection, ImportResult, Severity};
use railweave_openbve::{export_package, render_route, PackageOptions};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn usage() {
    eprintln!(
        "RailWeave\n\nUsage:\n  railweave scan <path>\n  railweave import <path> [-o <project.railweave.json>]\n  railweave compose <manifest.toml> [-o <composed.railweave.json>]\n  railweave export openbve <project.railweave.json> [-o <route.csv>]\n  railweave convert <source> --to openbve -o <package-dir> [--name <name>] [--adapter <program>] [--no-copy-native]\n"
    );
}

fn print_diagnostics(diagnostics: &[railweave_core::Diagnostic]) {
    for diagnostic in diagnostics {
        let label = match diagnostic.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        eprintln!("{label} [{}]: {}", diagnostic.code, diagnostic.message);
    }
}

fn scan(path: &Path) -> ExitCode {
    if !path.exists() {
        eprintln!("error: path does not exist: {}", path.display());
        return ExitCode::from(2);
    }

    let detections: Vec<Detection> = detect_all(path);

    println!("Source: {}", path.display());
    if detections.is_empty() {
        println!("Format: unknown");
        println!("No known simulator fingerprints were found.");
        return ExitCode::SUCCESS;
    }

    for (index, detection) in detections.iter().enumerate() {
        if index == 0 {
            println!(
                "Best match: {} ({}%)",
                detection.format, detection.confidence
            );
        } else {
            println!(
                "Alternative: {} ({}%)",
                detection.format, detection.confidence
            );
        }

        for evidence in &detection.evidence {
            println!("  - {evidence}");
        }
    }

    ExitCode::SUCCESS
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create output directory {}: {error}",
            parent.display()
        )
    })
}

fn write_result(result: &ImportResult, output: Option<&Path>, verb: &str) -> ExitCode {
    let json = match serde_json::to_string_pretty(result) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("error: failed to serialize IR: {error}");
            return ExitCode::from(1);
        }
    };

    if let Some(output) = output {
        if let Err(error) = ensure_parent(output) {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
        if let Err(error) = fs::write(output, format!("{json}\n")) {
            eprintln!("error: failed to write {}: {error}", output.display());
            return ExitCode::from(1);
        }

        println!(
            "{verb} {} nodes, {} edges, {} stations, {} assets, {} vehicles, {} consists -> {}",
            result.project.network.nodes.len(),
            result.project.network.edges.len(),
            result.project.stations.len(),
            result.project.assets.len(),
            result.project.vehicles.len(),
            result.project.consists.len(),
            output.display()
        );
        print_diagnostics(&result.diagnostics);
    } else {
        println!("{json}");
    }

    ExitCode::SUCCESS
}

fn convert(
    path: &Path,
    output: &Path,
    name: Option<String>,
    copy_native: bool,
    adapter: Option<&Path>,
) -> ExitCode {
    let imported = match adapter.map_or_else(
        || import_path(path),
        |adapter| import_external(adapter, path),
    ) {
        Ok(imported) => imported,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    let options = PackageOptions {
        name,
        copy_native_openbve_train: copy_native,
    };
    let package = match export_package(&imported.project, output, &options) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    println!(
        "Converted {} nodes, {} edges, {} vehicles -> OpenBVE package {}",
        imported.project.network.nodes.len(),
        imported.project.network.edges.len(),
        imported.project.vehicles.len(),
        package.root.display()
    );
    println!("  route: {}", package.route_path.display());
    println!("  train: {}", package.train_path.display());
    println!("  report: {}", package.manifest_path.display());
    print_diagnostics(&imported.diagnostics);
    print_diagnostics(&package.diagnostics);
    if imported
        .diagnostics
        .iter()
        .chain(package.diagnostics.iter())
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn import(path: &Path, output: Option<&Path>) -> ExitCode {
    let imported = match import_path(path) {
        Ok(imported) => imported,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    write_result(&imported, output, "Imported")
}

fn compose(path: &Path, output: Option<&Path>) -> ExitCode {
    let composed = match compose_manifest(path) {
        Ok(composed) => composed,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    write_result(&composed, output, "Composed")
}

fn export_openbve(path: &Path, output: Option<&Path>) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("error: failed to read {}: {error}", path.display());
            return ExitCode::from(1);
        }
    };
    let imported: ImportResult = match serde_json::from_str(&text) {
        Ok(imported) => imported,
        Err(error) => {
            eprintln!(
                "error: failed to parse RailWeave IR {}: {error}",
                path.display()
            );
            return ExitCode::from(1);
        }
    };
    let exported = match render_route(&imported.project) {
        Ok(exported) => exported,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };

    if let Some(output) = output {
        if let Err(error) = ensure_parent(output) {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
        if let Err(error) = fs::write(output, &exported.csv) {
            eprintln!("error: failed to write {}: {error}", output.display());
            return ExitCode::from(1);
        }
        println!("Exported OpenBVE route -> {}", output.display());
        print_diagnostics(&imported.diagnostics);
        print_diagnostics(&exported.diagnostics);
    } else {
        print!("{}", exported.csv);
    }

    ExitCode::SUCCESS
}

fn parse_output(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
) -> Result<Option<std::ffi::OsString>, ExitCode> {
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "-o" | "--output" => {
                let Some(value) = args.next() else {
                    eprintln!("error: {} requires a path", arg.to_string_lossy());
                    return Err(ExitCode::from(2));
                };
                output = Some(value);
            }
            other => {
                eprintln!("error: unknown option: {other}");
                return Err(ExitCode::from(2));
            }
        }
    }
    Ok(output)
}

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        usage();
        return ExitCode::from(2);
    };

    match command.to_string_lossy().as_ref() {
        "scan" => {
            let Some(path) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            if args.next().is_some() {
                eprintln!("error: scan accepts exactly one path");
                return ExitCode::from(2);
            }
            scan(Path::new(&path))
        }
        "import" => {
            let Some(path) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            let output = match parse_output(&mut args) {
                Ok(output) => output,
                Err(code) => return code,
            };
            import(Path::new(&path), output.as_deref().map(Path::new))
        }
        "compose" => {
            let Some(path) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            let output = match parse_output(&mut args) {
                Ok(output) => output,
                Err(code) => return code,
            };
            compose(Path::new(&path), output.as_deref().map(Path::new))
        }
        "export" => {
            let Some(target) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            if target.to_string_lossy().to_ascii_lowercase() != "openbve" {
                eprintln!(
                    "error: unsupported export target: {}",
                    target.to_string_lossy()
                );
                return ExitCode::from(2);
            }
            let Some(path) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            let output = match parse_output(&mut args) {
                Ok(output) => output,
                Err(code) => return code,
            };
            export_openbve(Path::new(&path), output.as_deref().map(Path::new))
        }
        "convert" => {
            let Some(path) = args.next() else {
                usage();
                return ExitCode::from(2);
            };
            let mut output = None;
            let mut name = None;
            let mut target = None;
            let mut copy_native = true;
            let mut adapter = None;
            while let Some(option) = args.next() {
                match option.to_string_lossy().as_ref() {
                    "-o" | "--output" => {
                        let Some(value) = args.next() else {
                            eprintln!("error: {} requires a path", option.to_string_lossy());
                            return ExitCode::from(2);
                        };
                        output = Some(value);
                    }
                    "--name" => {
                        let Some(value) = args.next() else {
                            eprintln!("error: --name requires a value");
                            return ExitCode::from(2);
                        };
                        name = Some(value.to_string_lossy().into_owned());
                    }
                    "--adapter" => {
                        let Some(value) = args.next() else {
                            eprintln!("error: --adapter requires an executable path");
                            return ExitCode::from(2);
                        };
                        adapter = Some(value);
                    }
                    "--to" => {
                        let Some(value) = args.next() else {
                            eprintln!("error: --to requires a target");
                            return ExitCode::from(2);
                        };
                        target = Some(value.to_string_lossy().to_ascii_lowercase());
                    }
                    "--no-copy-native" => copy_native = false,
                    other => {
                        eprintln!("error: unknown option: {other}");
                        return ExitCode::from(2);
                    }
                }
            }
            if target.as_deref() != Some("openbve") {
                eprintln!("error: convert currently requires --to openbve");
                return ExitCode::from(2);
            }
            let Some(output) = output else {
                eprintln!("error: convert requires -o <package-dir>");
                return ExitCode::from(2);
            };
            convert(
                Path::new(&path),
                Path::new(&output),
                name,
                copy_native,
                adapter.as_deref().map(Path::new),
            )
        }
        "-h" | "--help" | "help" => {
            usage();
            ExitCode::SUCCESS
        }
        "-V" | "--version" | "version" => {
            println!("railweave {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command: {other}");
            usage();
            ExitCode::from(2)
        }
    }
}
