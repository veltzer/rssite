//! End-to-end tests: one test binary, so the helpers in `common` are all
//! used somewhere and the dead-code lint stays honest.

mod common;

#[path = "tests_mod/build.rs"]
mod build;
#[path = "tests_mod/cli.rs"]
mod cli;
#[path = "tests_mod/features.rs"]
mod features;
#[path = "tests_mod/model.rs"]
mod model;
#[path = "tests_mod/serve.rs"]
mod serve;
#[path = "tests_mod/templates.rs"]
mod templates;
