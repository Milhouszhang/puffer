use super::*;

#[test]
fn workflows_new_reports_removed_native_drafts() {
    let tempdir = tempdir().unwrap();
    let paths = ConfigPaths::discover(tempdir.path());
    ensure_workspace_dirs(&paths).unwrap();
    let session_store = SessionStore::from_paths(&paths).unwrap();
    let session = session_store
        .create_session(tempdir.path().to_path_buf())
        .unwrap();
    let mut state = AppState::new(
        PufferConfig::default(),
        tempdir.path().to_path_buf(),
        session,
    );

    dispatch_command(
        &mut state,
        &supported_commands(),
        &LoadedResources::default(),
        &mut ProviderRegistry::new(),
        &mut AuthStore::default(),
        &session_store,
        "/workflows new Hi-Archive telegram-user hi",
    )
    .unwrap();

    let text = &state.transcript.last().unwrap().text;
    assert!(text.contains("Native workflow drafts were removed."));
    assert!(text.contains("configured workflow runtime"));
}
