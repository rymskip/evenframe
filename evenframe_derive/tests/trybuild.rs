#[test]
fn compile_tests() {
    let t = trybuild::TestCases::new();

    // Tests that should compile successfully
    t.pass("tests/ui/pass/*.rs");

    // Tests that should fail to compile with expected error messages
    t.compile_fail("tests/ui/fail/*.rs");
}
