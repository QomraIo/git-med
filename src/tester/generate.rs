pub mod sample;

use sample::{brats, dicom, smart};
use std::path::Path;

pub fn generate(args: &[String]) {
    if args.len() < 4 {
        println!(
            "Usage: git-med generate <brats|dicom|smart|all> <output_dir> [n_subjects] [study_per_subjects] [file_per_study/series]"
        );
        return;
    }

    let target_type = &args[2];
    let output_dir = Path::new(&args[3]);

    let parse_arg = |idx: usize, default: usize| -> usize {
        args.get(idx)
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    };

    match target_type.as_str() {
        "brats" => {
            let n_subjects = parse_arg(4, 1);
            let study_per_subjects = parse_arg(5, 0);
            let file_per_study = parse_arg(6, 5);
            brats(output_dir, n_subjects, study_per_subjects, file_per_study);
            println!(
                "Generated BraTS sample dataset in {:?} (subjects: {}, studies: {}, files: {})",
                output_dir, n_subjects, study_per_subjects, file_per_study
            );
        }
        "dicom" => {
            let n_subjects = parse_arg(4, 1);
            let study_per_subjects = parse_arg(5, 1);
            let series_per_study = parse_arg(6, 1);
            let file_per_series = parse_arg(7, 1);
            dicom(
                output_dir,
                n_subjects,
                study_per_subjects,
                series_per_study,
                file_per_series,
            );
            println!(
                "Generated DICOM sample dataset in {:?} (subjects: {}, studies: {}, series: {}, files: {})",
                output_dir, n_subjects, study_per_subjects, series_per_study, file_per_series
            );
        }
        "smart" => {
            let n_subjects = parse_arg(4, 1);
            let study_per_subjects = parse_arg(5, 1);
            let series_per_study = parse_arg(6, 1);
            let file_per_series = parse_arg(7, 1);
            let tasks: Vec<String> = if let Some(t) = args.get(8) {
                t.split(',').map(|s| s.trim().to_string()).collect()
            } else {
                vec![]
            };
            let tasks_ref: Vec<&str> = tasks.iter().map(|s| s.as_str()).collect();
            smart(
                output_dir,
                &tasks_ref,
                n_subjects,
                study_per_subjects,
                series_per_study,
                file_per_series,
            );
            println!(
                "Generated Smart sample dataset in {:?} (tasks: data [clinic, raw], segmentation [consensus, model_v1]; subjects: {}, studies: {}, series: {}, files: {})",
                output_dir, n_subjects, study_per_subjects, series_per_study, file_per_series
            );
        }
        "all" => {
            let n_subjects = parse_arg(4, 1);
            let study_per_subjects = parse_arg(5, 1);
            let series_per_study = parse_arg(6, 1);
            let file_per_series = parse_arg(7, 1);
            let file_per_study = parse_arg(6, 5);

            let brats_dir = output_dir.join("brats");
            let dicom_dir = output_dir.join("dicom");
            let smart_dir = output_dir.join("smart");

            let brats_study = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
            brats(&brats_dir, n_subjects, brats_study, file_per_study);
            dicom(
                &dicom_dir,
                n_subjects,
                study_per_subjects,
                series_per_study,
                file_per_series,
            );
            smart(
                &smart_dir,
                &[],
                n_subjects,
                study_per_subjects,
                series_per_study,
                file_per_series,
            );
            println!(
                "Generated all sample datasets (brats, dicom, smart) in {:?}",
                output_dir
            );
        }
        _ => println!(
            "Unknown generate type: {}. Available: brats, dicom, smart, all",
            target_type
        ),
    }
}
