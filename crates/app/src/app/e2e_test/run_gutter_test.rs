//! The run gutter's ▶ (IntelliJ's own "run this `main`" affordance): that
//! it appears exactly on the lines that declare an entry point, and that
//! clicking it reaches the real Run path.
//!
//! The click is deliberately exercised against a project with *no build
//! tool* (a bare temp directory of `.java` files, which is what `E2e::
//! launch` makes): the whole wiring — marker, click, request, project
//! lookup, run — runs for real up to the point where a real `mvn`/`gradle`
//! would be spawned, and stops there with the same error a user gets. A
//! test that actually launched a build would depend on a Maven install and
//! a network.

use super::common_test::{E2e, MAIN_JAVA};
use fg_i18n::{msg, t};

const WITH_MAIN: &str =
    "public class App {\n    public static void main(String[] args) {\n        System.out.println(\"hi\");\n    }\n}\n";

#[test]
fn a_class_with_a_main_gets_a_run_marker() {
    let mut app = E2e::launch(&[("App.java", WITH_MAIN)]);
    app.click_tree("App.java");

    assert!(
        app.shows(&msg::run_main_class("App")),
        "the ▶ names the class it would run"
    );
}

#[test]
fn a_class_without_a_main_gets_no_marker() {
    let mut app = E2e::launch(&[("Main.java", MAIN_JAVA)]);
    app.click_tree("Main.java");

    assert!(
        !app.shows(&msg::run_main_class("Main")),
        "a class with no entry point has nothing to run"
    );
}

#[test]
fn clicking_the_marker_reaches_the_real_run_path() {
    let mut app = E2e::launch(&[("App.java", WITH_MAIN)]);
    app.click_tree("App.java");

    app.click(&msg::run_main_class("App"));

    assert!(
        app.shows(t().errors.no_build_tool_detected),
        "the click must reach the same Run machinery the menu uses, which is what reports this"
    );
}
