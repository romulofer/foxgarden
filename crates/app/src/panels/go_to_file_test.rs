
use super::*;

#[test]
fn fuzzy_score_matches_a_contiguous_substring() {
    assert!(fuzzy_score("UserController.java", "User").is_some());
}

#[test]
fn fuzzy_score_matches_a_scattered_subsequence_across_a_path() {
    // "contuser" -> c-o-n-t-r-o-l-l-e-r-s/-U-s-e-r-...
    assert!(fuzzy_score("controllers/UserController.java", "contuser").is_some());
}

#[test]
fn fuzzy_score_is_case_insensitive() {
    assert!(fuzzy_score("UserController.java", "usercontroller").is_some());
}

#[test]
fn fuzzy_score_returns_none_when_chars_are_out_of_order() {
    // "reuse" requires r,e,u,s,e in order; "user" alone has no "r" at
    // all before the rest, so this simply shouldn't match.
    assert_eq!(fuzzy_score("User.java", "ruesj"), None);
}

#[test]
fn fuzzy_score_empty_query_matches_everything_with_zero_score() {
    assert_eq!(fuzzy_score("anything.java", ""), Some(0));
}

#[test]
fn fuzzy_score_ranks_a_contiguous_match_above_a_scattered_one() {
    let contiguous = fuzzy_score("User.java", "User").unwrap();
    let scattered = fuzzy_score("UnrelatedStuffEndingR.java", "User").unwrap();
    assert!(
        contiguous > scattered,
        "contiguous ({contiguous}) should outscore scattered ({scattered})"
    );
}

#[test]
fn fuzzy_score_ranks_an_earlier_match_above_a_later_one() {
    let earlier = fuzzy_score("UserController.java", "User").unwrap();
    let later = fuzzy_score("AbstractBaseUserController.java", "User").unwrap();
    assert!(
        earlier > later,
        "earlier match ({earlier}) should outscore later match ({later})"
    );
}

fn file(path: &str) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        name: PathBuf::from(path).file_name().unwrap().to_string_lossy().into_owned(),
        kind: FileKind::File,
        children: Vec::new(),
    }
}

fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        name: PathBuf::from(path).file_name().unwrap().to_string_lossy().into_owned(),
        kind: FileKind::Dir,
        children,
    }
}

#[test]
fn all_files_flattens_a_nested_tree_skipping_directories() {
    let tree = dir(
        "/root",
        vec![
            file("/root/Main.java"),
            dir("/root/pkg", vec![file("/root/pkg/A.java"), file("/root/pkg/B.java")]),
        ],
    );
    let mut out = Vec::new();
    all_files(&tree, &mut out);

    assert_eq!(
        out,
        vec![
            PathBuf::from("/root/Main.java"),
            PathBuf::from("/root/pkg/A.java"),
            PathBuf::from("/root/pkg/B.java"),
        ]
    );
}

#[test]
fn matching_files_ranks_and_filters_by_the_query() {
    let root = PathBuf::from("/root");
    let tree = dir(
        "/root",
        vec![
            file("/root/UserController.java"),
            dir("/root/controllers", vec![file("/root/controllers/UserController.java")]),
            file("/root/Unrelated.java"),
        ],
    );

    let results = matching_files(&tree, &root, "usercontroller");

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|p| p.to_string_lossy().contains("UserController")));
}

#[test]
fn matching_files_with_empty_query_returns_every_file() {
    let root = PathBuf::from("/root");
    let tree = dir("/root", vec![file("/root/A.java"), file("/root/B.java")]);

    assert_eq!(matching_files(&tree, &root, "").len(), 2);
}

#[test]
fn toggle_opens_and_resets_query_and_selection() {
    let mut switcher = GoToFileState {
        open: false,
        query: "leftover".to_string(),
        selected: 3,
    };

    switcher.toggle();

    assert!(switcher.open);
    assert!(switcher.query.is_empty());
    assert_eq!(switcher.selected, 0);
}

#[test]
fn toggle_twice_closes_it_again() {
    let mut switcher = GoToFileState::default();
    switcher.toggle();
    switcher.toggle();
    assert!(!switcher.open);
}
