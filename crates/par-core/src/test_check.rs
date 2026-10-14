use std::sync::mpsc;

use par_runtime::readback::Handle;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckResult {
    pub description: String,
    pub passed: bool,
}

pub async fn provide_test(handle: Handle, sender: mpsc::Sender<CheckResult>) {
    provide_test_inner(handle, sender);
}

fn provide_test_inner(handle: Handle, sender: mpsc::Sender<CheckResult>) {
    handle.provide_box(move |mut handle| {
        let sender = sender.clone();
        async move {
            match handle.case().await.as_str() {
                "check" => {
                    let description = handle.receive().string().await.as_str().to_string();
                    let mut bool_handle = handle.receive();

                    let passed = match bool_handle.case().await.as_str() {
                        "true" => {
                            bool_handle.continue_();
                            true
                        }
                        "false" => {
                            bool_handle.continue_();
                            false
                        }
                        variant => {
                            panic!(
                                "Test.check expected Bool, got unexpected variant: {}",
                                variant
                            );
                        }
                    };

                    let result = CheckResult {
                        description,
                        passed,
                    };
                    let _ = sender.send(result);

                    provide_test_inner(handle, sender);
                }
                other => {
                    panic!("Unexpected method call on Test: {}", other);
                }
            }
        }
    })
}
