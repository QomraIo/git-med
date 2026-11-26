use std::fs;
use std::path::Path;

pub const BRATS_SAMPLE: &[u8] = include_bytes!("data/BraTS.nii.gz");
pub const DICOM_SAMPLE: &[u8] =
    include_bytes!("data/1.3.12.2.1107.5.2.19.45594.2018071915225876779003334.dcm");

pub fn brats(root: &Path, n_subjects: usize, study_per_subjects: usize, file_per_study: usize) {
    let modalities = ["t1", "t1ce", "t2", "flair", "seg"];

    for s in 1..=n_subjects {
        let subject_name = format!("subject_{:03}", s);

        if study_per_subjects == 0 {
            let subject_dir = root.join(&subject_name);
            fs::create_dir_all(&subject_dir).expect("Failed to create subject directory");

            for f in 0..file_per_study {
                let modality = if f < modalities.len() {
                    modalities[f].to_string()
                } else {
                    format!("modality_{}", f)
                };
                let file_name = format!("{}_{}.nii.gz", subject_name, modality);
                let target = subject_dir.join(&file_name);
                fs::write(&target, BRATS_SAMPLE).expect("Failed to write BraTS sample file");

                let (source, task) = if modality == "seg" {
                    ("consensus", "segmentation")
                } else {
                    ("clinic", "data")
                };
                let sidecar_name = format!(".view.{}", file_name);
                let sidecar_target = subject_dir.join(sidecar_name);
                let sidecar_content = format!("{}\n{}\n", source, task);
                fs::write(&sidecar_target, sidecar_content).expect("Failed to write sidecar file");
            }
        } else {
            for st in 1..=study_per_subjects {
                let study_name = format!("study_{:03}", st);
                let study_dir = root.join(&subject_name).join(&study_name);
                fs::create_dir_all(&study_dir).expect("Failed to create study directory");

                for f in 0..file_per_study {
                    let modality = if f < modalities.len() {
                        modalities[f].to_string()
                    } else {
                        format!("modality_{}", f)
                    };
                    let file_name = format!("{}_{}_{}.nii.gz", subject_name, study_name, modality);
                    let target = study_dir.join(&file_name);
                    fs::write(&target, BRATS_SAMPLE).expect("Failed to write BraTS sample file");

                    let (source, task) = if modality == "seg" {
                        ("consensus", "segmentation")
                    } else {
                        ("clinic", "data")
                    };
                    let sidecar_name = format!(".view.{}", file_name);
                    let sidecar_target = study_dir.join(sidecar_name);
                    let sidecar_content = format!("{}\n{}\n", source, task);
                    fs::write(&sidecar_target, sidecar_content).expect("Failed to write sidecar file");
                }
            }
        }
    }
}

pub fn dicom(
    root: &Path,
    n_subjects: usize,
    study_per_subjects: usize,
    series_per_study: usize,
    file_per_series: usize,
) {
    for s in 1..=n_subjects {
        let subject_name = format!("subject_{:03}", s);
        for st in 1..=study_per_subjects {
            let study_name = format!("study_{:03}", st);
            for se in 1..=series_per_study {
                let series_name = format!("series_{:03}", se);
                let series_dir = root
                    .join(&subject_name)
                    .join(&study_name)
                    .join(&series_name);
                fs::create_dir_all(&series_dir).expect("Failed to create series directory");

                for f in 1..=file_per_series {
                    let file_name = format!("instance_{:03}.dcm", f);
                    let target = series_dir.join(&file_name);
                    fs::write(&target, DICOM_SAMPLE).expect("Failed to write DICOM sample file");

                    let (source, task) = ("clinic", "data");
                    let sidecar_name = format!(".view.{}", file_name);
                    let sidecar_target = series_dir.join(sidecar_name);
                    let sidecar_content = format!("{}\n{}\n", source, task);
                    fs::write(&sidecar_target, sidecar_content).expect("Failed to write sidecar file");
                }
            }
        }
    }
}

pub fn smart(
    root: &Path,
    tasks: &[&str],
    n_subjects: usize,
    study_per_subjects: usize,
    series_per_study: usize,
    file_per_series: usize,
) {
    let tasks_to_generate: Vec<&str> = if tasks.is_empty() {
        vec!["data", "segmentation"]
    } else {
        tasks.to_vec()
    };

    for task_spec in tasks_to_generate {
        let (task, sources): (&str, &[&str]) = match task_spec {
            "data" => ("data", &["clinic", "k-space"][..]),
            "clinic" | "data:clinic" => ("data", &["clinic"][..]),
            "k-space" | "data:k-space" => ("data", &["k-space"][..]),
            "segmentation" => ("segmentation", &["consensus", "model_v1"][..]),
            "consensus" | "segmentation:consensus" => ("segmentation", &["consensus"][..]),
            "model_v1" | "segmentation:model_v1" => ("segmentation", &["model_v1"][..]),
            "localisation" => ("localisation", &["annotator_1", "detector_model"][..]),
            _ => (task_spec, &["default_source"][..]),
        };

        for source in sources {
            for s in 1..=n_subjects {
                let subject_name = format!("subject_{:03}", s);
                for st in 1..=study_per_subjects {
                    let study_name = format!("study_{:03}", st);
                    for se in 1..=series_per_study {
                        let series_name = format!("series_{:03}", se);
                        let series_dir = root
                            .join(task)
                            .join(source)
                            .join(&subject_name)
                            .join(&study_name)
                            .join(&series_name);
                        fs::create_dir_all(&series_dir).expect("Failed to create series directory");

                        for f in 1..=file_per_series {
                            let (file_name, payload) = if task == "segmentation" {
                                (format!("instance_{:03}_seg.nii.gz", f), BRATS_SAMPLE)
                            } else {
                                (format!("instance_{:03}.dcm", f), DICOM_SAMPLE)
                            };
                            let target = series_dir.join(file_name);
                            fs::write(&target, payload).expect("Failed to write sample file");
                        }
                    }
                }
            }
        }
    }
}
