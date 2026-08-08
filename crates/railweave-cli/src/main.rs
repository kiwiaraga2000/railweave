use railweave_adapters::built_in_detectors;
use railweave_core::Detection;
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn usage() {
    eprintln!("RailWeave\n\nUsage:\n  railweave scan <path>\n");
}

fn scan(path: &Path) -> ExitCode {
    if !path.exists() {
        eprintln!("error: path does not exist: {}", path.display());
        return ExitCode::from(2);
    }

    let mut detections: Vec<Detection> = built_in_detectors()
        .into_iter()
        .map(|detector| detector.detect(path))
        .filter(|detection| detection.confidence > 0)
        .collect();

    detections.sort_by(|a, b| b.confidence.cmp(&a.confidence));

    println!("Source: {}", path.display());
    if detections.is_empty() {
        println!("Format: unknown");
        println!("No known simulator fingerprints were found.");
        return ExitCode::SUCCESS;
    }

    for (index, detection) in detections.iter().enumerate() {
        if index == 0 {
            println!("Best match: {} ({}%)", detection.format, detection.confidence);
        } else {
            println!("Alternative: {} ({}%)", detection.format, detection.confidence);
        }

        for evidence in &detection.evidence {
            println!("  - {evidence}");
        }
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
