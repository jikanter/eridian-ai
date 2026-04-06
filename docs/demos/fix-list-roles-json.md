# Fix: aichat --list-roles -o json missing builtin roles

*2026-04-06T15:46:00Z by Showboat 0.6.1*
<!-- showboat-id: 52141fe3-81ba-4788-80e0-ff7c7989ea79 -->

The bug was that Config::all_roles() only listed local roles when preparing the JSON output, skipping builtin roles like %code% and %shell%.

After applying the fix, all roles (both builtin and local) are correctly included in the JSON output.

```bash
./target/debug/aichat --list-roles -o json | jq -r '.[].name' | grep '^%'
```

```output
%code%
%create-prompt%
%create-title%
%explain-shell%
%functions%
%shell%
```

The existing BATS test has also been updated to verify this fix.

```bash
bats tests/cli/list-roles.sh
```

```output
1..2
ok 1 list roles returns some roles with json
ok 2 list roles -o json returns both builtin and custom roles
```
