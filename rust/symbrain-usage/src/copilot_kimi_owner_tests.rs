// Source-bound owner selection and native-platform filepath comparisons.
fn owner_matches_frozen_go(evidence: &frozen::Frozen) {
    let records: Vec<serde_json::Value> = evidence.documents("owner").as_array().unwrap().clone();
    assert_eq!(records.len(), 16, "all owner cases");
    assert_eq!(
        records
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        16
    );
    for record in &records {
        prepare(record);
        let before: BTreeMap<_, _> = record["file_sha256"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(path, hash)| {
                let bytes = std::fs::read(path).unwrap();
                assert_eq!(
                    format!("{:x}", Sha256::digest(&bytes)),
                    hash.as_str().unwrap()
                );
                (path, bytes)
            })
            .collect();
        let link = record["link"].as_str().unwrap();
        let target = std::fs::read_link(link).unwrap();
        assert_eq!(
            target,
            PathBuf::from(record["link_target"].as_str().unwrap())
        );
        let provider = if record["provider"] == "copilot" {
            super::copilot()
        } else {
            super::kimi()
        };
        let transport = Arc::new(Unauthorized(Mutex::new(Vec::new())));
        let report = Service::with_transport(vec![provider], transport.clone()).report();
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            record["report"],
            "{}: complete owner report",
            record["id"]
        );
        assert_eq!(
            serde_json::to_value(transport.0.lock().unwrap().clone()).unwrap(),
            normalized_requests(&record["requests"]),
            "{}: complete owner request bytes",
            record["id"]
        );
        for (path, bytes) in before {
            assert_eq!(
                std::fs::read(path).unwrap(),
                bytes,
                "{}: owner sources read-only",
                record["id"]
            );
        }
        assert_eq!(
            std::fs::read_link(link).unwrap(),
            target,
            "owner symlink read-only"
        );
    }
    paths_match_frozen_go(evidence);
}
fn decode_hex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0);
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}
fn paths_match_frozen_go(evidence: &frozen::Frozen) {
    let records: Vec<serde_json::Value> = evidence.documents("path").as_array().unwrap().clone();
    assert_eq!(records.len(), if cfg!(windows) { 21 } else { 22 });
    for record in &records {
        let bytes = decode_hex(record["base_hex"].as_str().unwrap());
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(bytes))
        };
        #[cfg(windows)]
        let path = PathBuf::from(String::from_utf8(bytes).unwrap());
        let clean = super::credential_path_from_units(&super::credential_clean_units(
            &super::credential_path_units(&path),
        ));
        let joined = super::credential_join(&path, record["child"].as_str().unwrap());
        #[cfg(unix)]
        let as_bytes = |path: &std::path::Path| {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(windows)]
        let as_bytes = |path: &std::path::Path| path.to_str().unwrap().as_bytes().to_vec();
        assert_eq!(
            hex(&as_bytes(&clean)),
            record["clean_hex"],
            "actual Go clean: {path:?}"
        );
        assert_eq!(
            hex(&as_bytes(&joined)),
            record["join_hex"],
            "actual Go join: {path:?}"
        );
    }
}
