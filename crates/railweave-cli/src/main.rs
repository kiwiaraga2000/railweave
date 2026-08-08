use railweave_adapters::{detect_all, import_path};
use railweave_core::{Detection, Severity};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn usage() {
    eprintln!(
        "RailWeave\n\nUsage:\n  railweave scan <path>\n  railweave import <path> [-o <project.railweave.json>]\n"
    );
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

fn import(path: &Path, output: Option<&Path>) -> ExitCode {
    let imported = match import_path(path) {
        Ok(imported) => imported,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };

    let json = match serde_json::to_string_pretty(&imported) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("error: failed to serialize IR: {error}");
            return ExitCode::from(1);
        }
    };

    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(error) = fs::create_dir_all(parent) {
                    eprintln!(
                        "error: failed to create output directory {}: {error}",
                        parent.display()
                    );
                    return ExitCode::from(1);
                }
            }
        }
        if let Err(error) = fs::write(output, format!("{json}\n")) {
            eprintln!("error: failed to write {}: {error}", output.display());
            return ExitCode::from(1);
        }

        println!(
            "Imported {} nodes, {} edges, {} assets -> {}",
            imported.project.network.nodes.len(),
            imported.project.network.edges.len(),
            imported.project.assets.len(),
            output.display()
        );
        for diagnostic in &imported.diagnostics {
            let label = match diagnostic.severity {
                Severity::Info => "info",
                Severity::Warning => "warning",
                Severity::Error => "error",
            };
            eprintln!("{label} [{}]: {}", diagnostic.code, diagnostic.message);
        }
    } else {
        println!("{json}");
    }

    ExitCode::SUCCESS
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

            let mut output = None;
            while let Some(arg) = args.next() {
                match arg.to_string_lossy().as_ref() {
                    "-o" | "--output" => {
                        let Some(value) = args.next() else {
                            eprintln!("error: {} requires a path", arg.to_string_lossy());
                            return ExitCode::from(2);
                        };
                        output = Some(value);
                    }
                    other => {
                        eprintln!("error: unknown import option: {other}");
                        return ExitCode::from(2);
                    }
                }
            }

            import(Path::new(&path), output.as_deref().map(Path::new))
        }
        "-h" | "--help" | "help" => {
            usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command: {other}");
            usage();
            ExitCode::from(2)
        }
    }
}
