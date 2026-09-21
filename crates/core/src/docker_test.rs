use super::*;

#[test]
fn has_dockerfile_is_true_only_with_a_real_file_at_the_root() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!has_dockerfile(dir.path()));
    std::fs::write(dir.path().join("Dockerfile"), "").unwrap();
    assert!(has_dockerfile(dir.path()));
}

#[test]
fn compose_file_prefers_compose_yaml_over_every_other_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("docker-compose.yml"), "").unwrap();
    std::fs::write(dir.path().join("docker-compose.yaml"), "").unwrap();
    std::fs::write(dir.path().join("compose.yml"), "").unwrap();
    std::fs::write(dir.path().join("compose.yaml"), "").unwrap();
    assert_eq!(compose_file(dir.path()), Some(dir.path().join("compose.yaml")));
}

#[test]
fn compose_file_falls_back_to_legacy_names_in_order() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("docker-compose.yml"), "").unwrap();
    std::fs::write(dir.path().join("docker-compose.yaml"), "").unwrap();
    assert_eq!(compose_file(dir.path()), Some(dir.path().join("docker-compose.yaml")));
}

#[test]
fn compose_file_is_none_with_no_compose_file_present() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(compose_file(dir.path()), None);
}

#[test]
fn docker_build_command_tags_and_sets_the_build_context_to_the_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let command = docker_build_command(dir.path());
    assert_eq!(command.get_program(), "docker");
    let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args[0], "build");
    assert!(args.contains(&"-t".to_string()));
    assert_eq!(command.get_current_dir(), Some(dir.path()));
}

#[test]
fn docker_image_tag_sanitizes_a_directory_name_with_spaces_and_accents() {
    let dir = tempfile::tempdir().unwrap().path().join("Meu Projeto Ção");
    std::fs::create_dir_all(&dir).unwrap();
    let tag = docker_image_tag(&dir);
    assert!(tag.starts_with("foxgarden-"));
    assert!(
        tag.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    );
}

#[test]
fn docker_run_command_uses_the_same_tag_docker_build_command_would() {
    let dir = tempfile::tempdir().unwrap();
    let build = docker_build_command(dir.path());
    let run = docker_run_command(dir.path(), "foxgarden-x-1");
    let build_args: Vec<_> = build.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let run_args: Vec<_> = run.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let tag = &build_args[build_args.iter().position(|a| a == "-t").unwrap() + 1];
    assert!(run_args.contains(tag));
}

#[test]
fn docker_run_command_names_the_container_so_it_can_be_stopped_later() {
    let dir = tempfile::tempdir().unwrap();
    let run = docker_run_command(dir.path(), "foxgarden-demo-123");
    let args: Vec<_> = run.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let name = &args[args.iter().position(|a| a == "--name").expect("--name is passed") + 1];
    assert_eq!(name, "foxgarden-demo-123");
    assert!(args.contains(&"--rm".to_string()), "still one-shot cleanup on exit");
}

#[test]
fn container_name_is_unique_valid_and_tag_prefixed() {
    let dir = tempfile::tempdir().unwrap();
    let first = container_name(dir.path());
    assert!(first.starts_with("foxgarden-"));
    assert!(
        first
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    );
    // The whole point of the suffix: two names generated a moment apart
    // don't collide, so `docker run --name` won't reject the second.
    std::thread::sleep(std::time::Duration::from_millis(2));
    assert_ne!(first, container_name(dir.path()));
}

#[test]
fn docker_stop_command_stops_the_named_container() {
    let command = docker_stop_command("foxgarden-demo-123");
    assert_eq!(command.get_program(), "docker");
    let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args, vec!["stop", "foxgarden-demo-123"]);
}

#[test]
fn docker_compose_down_command_targets_the_file_and_its_own_directory() {
    let dir = tempfile::tempdir().unwrap();
    let compose = dir.path().join("compose.yaml");
    std::fs::write(&compose, "").unwrap();
    let command = docker_compose_down_command(&compose);
    assert_eq!(command.get_current_dir(), Some(dir.path()));
    let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args, vec!["compose", "-f", compose.to_str().unwrap(), "down"]);
}

#[test]
fn docker_compose_up_command_sets_cwd_to_the_compose_files_own_directory() {
    let dir = tempfile::tempdir().unwrap();
    let compose = dir.path().join("compose.yaml");
    std::fs::write(&compose, "").unwrap();
    let command = docker_compose_up_command(&compose);
    assert_eq!(command.get_current_dir(), Some(dir.path()));
    let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args, vec!["compose", "-f", compose.to_str().unwrap(), "up", "--build"]);
}
