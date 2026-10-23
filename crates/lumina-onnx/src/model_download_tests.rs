use super::*;
use std::{
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Mock {
    data: Vec<u8>,
    content_length: Option<u64>,
    failure: MockFailure,
    occupy_destination: Option<PathBuf>,
    opens: AtomicUsize,
}

#[derive(Clone, Copy)]
enum MockFailure {
    None,
    Http,
    Interrupted,
}

impl ArtifactTransport for Mock {
    fn open(&self, _: &str) -> Result<DownloadResponse, String> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        if matches!(self.failure, MockFailure::Http) {
            return Err("HTTP status 503".into());
        }
        if let Some(destination) = &self.occupy_destination {
            fs::create_dir_all(destination).map_err(|error| error.to_string())?;
            fs::write(destination.join("concurrent-owner"), b"preserve me")
                .map_err(|error| error.to_string())?;
        }
        let body: Box<dyn Read> = match self.failure {
            MockFailure::Interrupted => Box::new(Interrupted(Cursor::new(self.data.clone()))),
            _ => Box::new(Cursor::new(self.data.clone())),
        };
        Ok(DownloadResponse {
            content_length: self.content_length,
            body,
        })
    }
}

struct Interrupted<R>(R);
impl<R: Read> Read for Interrupted<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.0.read(buffer)?;
        if count == 0 {
            Err(io::Error::other("interrupted read"))
        } else {
            Ok(count)
        }
    }
}

fn setup(data: Vec<u8>, cap: u64) -> (PackageDescriptor, LicenseAcceptance, PathBuf, Mock) {
    let license = "terms v1";
    let package = PackageDescriptor {
        model_id: "safe-model".into(),
        version: "1".into(),
        filename: "model.onnx".into(),
        artifact_url: "https://example.test/model".into(),
        provenance_url: "https://example.test/source".into(),
        license_id: "MIT".into(),
        license_text: license.into(),
        sha256: format!("sha256:{}", digest(&data)),
        max_bytes: cap,
    };
    let consent = LicenseAcceptance::accept_exact("MIT", license);
    let root = std::env::temp_dir().join(format!("lumina-model-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let transport = Mock {
        content_length: Some(data.len() as u64),
        data,
        failure: MockFailure::None,
        occupy_destination: None,
        opens: AtomicUsize::new(0),
    };
    (package, consent, root, transport)
}

#[test]
fn concurrent_destination_is_never_replaced_and_staging_is_cleaned() {
    let (package, consent, root, mut transport) = setup(b"abc".to_vec(), 10);
    let destination = root.join(&package.model_id).join(&package.version);
    transport.occupy_destination = Some(destination.clone());
    assert!(install(&package, &consent, &root, &transport).is_err());
    assert_eq!(
        fs::read(destination.join("concurrent-owner")).unwrap(),
        b"preserve me"
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    assert_eq!(
        fs::read_dir(root.join(&package.model_id)).unwrap().count(),
        1
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn dangling_destination_symlink_is_occupied_and_prevents_transport() {
    use std::os::unix::fs::symlink;

    let (package, _, root, transport) = setup(b"abc".to_vec(), 10);
    let destination = root.join(&package.model_id).join(&package.version);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    symlink(root.join("missing-target"), &destination).unwrap();

    assert_eq!(
        destination_status(&destination, &package).unwrap(),
        Some(false)
    );
    assert_eq!(transport.opens.load(Ordering::SeqCst), 0);
    assert!(install(
        &package,
        &LicenseAcceptance::accept_exact(&package.license_id, &package.license_text),
        &root,
        &transport,
    )
    .is_err());
    assert!(fs::symlink_metadata(&destination)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(transport.opens.load(Ordering::SeqCst), 0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_consent_validation_and_urls_precede_transport() {
    let (package, _, root, transport) = setup(b"abc".to_vec(), 10);
    let wrong = LicenseAcceptance::accept_exact("MIT", "other");
    assert!(install(&package, &wrong, &root, &transport).is_err());
    assert_eq!(transport.opens.load(Ordering::SeqCst), 0);
    for (field, value) in [
        ("artifact", "http://example.test/model"),
        ("provenance", "https://user@example.test/source"),
    ] {
        let mut bad = package.clone();
        if field == "artifact" {
            bad.artifact_url = value.into();
        } else {
            bad.provenance_url = value.into();
        }
        assert!(validate(&bad).is_err());
    }
    let mut bad = package.clone();
    bad.sha256 = "pending-integration".into();
    assert!(validate(&bad).is_err());
    bad = package.clone();
    bad.license_id = "not a license?".into();
    assert!(validate(&bad).is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn streaming_pin_metadata_idempotency_and_occupied_destination() {
    let (package, consent, root, transport) = setup(vec![7; 200_000], 300_000);
    let destination = install(&package, &consent, &root, &transport).unwrap();
    assert_eq!(
        fs::read(destination.join(&package.filename)).unwrap(),
        transport.data
    );
    assert_eq!(
        fs::read(destination.join("LICENSE")).unwrap(),
        package.license_text.as_bytes()
    );
    let metadata: InstalledMetadata =
        serde_json::from_slice(&fs::read(destination.join("metadata.json")).unwrap()).unwrap();
    assert_eq!(metadata.model_id, package.model_id);
    assert_eq!(
        install(&package, &consent, &root, &transport).unwrap(),
        destination
    );
    let mut other = package.clone();
    other.sha256 = format!("sha256:{}", digest(b"different"));
    assert!(install(&other, &consent, &root, &transport).is_err());
    assert_eq!(
        fs::read(destination.join(&package.filename)).unwrap(),
        transport.data
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn declared_and_streamed_oversize_http_failure_hash_mismatch_and_interruption_clean_stage() {
    for case in 0..5 {
        let (mut package, consent, root, mut transport) = setup(b"abc".to_vec(), 10);
        match case {
            0 => transport.content_length = Some(11),
            1 => transport.failure = MockFailure::Http,
            2 => {
                package.max_bytes = 2;
                transport.content_length = None;
            }
            3 => package.sha256 = format!("sha256:{}", digest(b"wrong")),
            4 => transport.failure = MockFailure::Interrupted,
            _ => unreachable!(),
        }
        assert!(install(&package, &consent, &root, &transport).is_err());
        assert!(!root.join(&package.model_id).exists());
        if root.exists() {
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        }
        let _ = fs::remove_dir_all(root);
    }
}
