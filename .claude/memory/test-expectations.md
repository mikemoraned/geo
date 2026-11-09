# A test states what it expects

A test writes its expected values as literals: a lifetime as "ten minutes", a redis key as
`"lookout-session:{id}"`, a cookie name as `"__Host-lookout-session"`. It never builds them through
the constants or helpers of the code under test.

A test that reads the implementation's constant or calls its helper follows any change to it. A
mutant that changes the constant moves the test with it, so the mutant survives. On 8 November
2026, mutation testing found the lookout auth store's lifetime tests counting time with the
lifetime constants they checked. On 9 November 2026, a `/simplify` review proposed building the
tests' redis keys through the stores' key helpers, and the change was undone for the same reason.

A literal also guards what the value names outside the code. A redis key names data that
production already holds, so renaming it orphans that data. A test that fails on the rename makes
the rename a deliberate change rather than a silent one.

A shared test helper is fine where it builds inputs, such as a registered passkey or a signed-in
browser. The rule covers the expected side of an assertion.
