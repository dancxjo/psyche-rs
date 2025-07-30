pub mod imp {
    use crate::{FaceEntry, Recognizer};
    use anyhow::Context;
    use async_trait::async_trait;
    use opencv::img_hash::p_hash;
    use opencv::{core, imgcodecs, imgproc, objdetect};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    pub struct OpenCVRecognizer {
        memory_sock: PathBuf,
    }

    impl OpenCVRecognizer {
        pub fn new(memory_sock: PathBuf) -> Self {
            Self { memory_sock }
        }
        async fn save_face(&self, entry: &FaceEntry) -> anyhow::Result<()> {
            let val = serde_json::to_value(entry)?;
            super::super::memory::memorize(&self.memory_sock, "face", val).await
        }

        async fn alias_map(&self) -> anyhow::Result<std::collections::HashMap<Uuid, String>> {
            let aliases = super::super::memory::list(&self.memory_sock, "face_alias").await?;
            let mut map = std::collections::HashMap::new();
            for alias in aliases {
                if let (Some(id), Some(name)) = (alias.get("id"), alias.get("name")) {
                    if let (Some(id), Some(name)) = (id.as_str(), name.as_str()) {
                        map.insert(Uuid::parse_str(id)?, name.to_string());
                    }
                }
            }
            Ok(map)
        }
    }

    fn face_hash(face: &core::Mat) -> anyhow::Result<Vec<f32>> {
        let mut hash = core::Mat::default();
        p_hash(face, &mut hash)?;
        let slice = hash.data_typed::<u8>()?;
        Ok(slice.iter().map(|b| *b as f32 / 255.0).collect())
    }

    #[async_trait]
    impl Recognizer for OpenCVRecognizer {
        async fn recognize(&self, img: &[u8]) -> anyhow::Result<Vec<String>> {
            let alias_map = self.alias_map().await.unwrap_or_default();
            let mat = imgcodecs::imdecode(&core::Vector::from_slice(img), imgcodecs::IMREAD_COLOR)?;
            let mut gray = core::Mat::default();
            imgproc::cvt_color(&mat, &mut gray, imgproc::COLOR_BGR2GRAY, 0)?;
            let mut detector = objdetect::CascadeClassifier::new(&format!(
                "{}/haarcascades/haarcascade_frontalface_default.xml",
                opencv::core::get_include_str().unwrap_or("/usr/share/opencv4")
            ))?;
            let mut faces_rects = opencv::types::VectorOfRect::new();
            detector.detect_multi_scale(
                &gray,
                &mut faces_rects,
                1.1,
                3,
                0,
                core::Size::new(30, 30),
                core::Size::new(0, 0),
            )?;
            if faces_rects.len() == 0 {
                return Ok(Vec::new());
            }
            let mut names = Vec::new();
            for rect in faces_rects {
                let face = core::Mat::roi(&mat, rect)?;
                let emb = face_hash(&face)?;
                let results =
                    super::super::memory::query_vector(&self.memory_sock, "face", &emb, 1).await?;
                let mut found = false;
                if let Some(top) = results.first() {
                    if let (Some(id), Some(score)) = (top.get("id"), top.get("score")) {
                        if let (Some(id), Some(score)) = (id.as_str(), score.as_f64()) {
                            if score < 0.1 {
                                if let Ok(uuid) = Uuid::parse_str(id) {
                                    if let Some(name) = alias_map.get(&uuid) {
                                        names.push(name.clone());
                                    } else {
                                        names.push("Unknown".into());
                                    }
                                    found = true;
                                }
                            }
                        }
                    }
                }
                if !found {
                    let id = Uuid::new_v4();
                    let entry = FaceEntry {
                        id,
                        embedding: emb.clone(),
                        name: None,
                    };
                    self.save_face(&entry).await.ok();
                    names.push(format!("Stranger {}", id.simple()));
                }
            }
            Ok(names)
        }
    }
}

pub use imp::OpenCVRecognizer;
