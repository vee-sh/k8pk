//! Long-form help: workflows and command map (see `k8pk guide`).

/// Print the interactive guide to stdout (for humans; use `k8pk --help` for flags).
pub fn print_guide() {
    print!("{}", GUIDE_TEXT);
}

const GUIDE_TEXT: &str = r#"k8pk — workflows and command map
=================================

Common tasks
------------
  Pick cluster, open a shell (default)           k8pk          or  kpick  (shell alias)
  Switch context (named)                       k8pk ctx NAME   or  kctx NAME
  Previous context / namespace                 k8pk ctx -      /  k8pk ns -
  List contexts (with source files)            k8pk contexts --path
  Remove a dead / unwanted context             k8pk rm NAME    (auto-finds kubeconfig file)
  Pick context to remove (multi-select)        k8pk rm
  Skip confirm when scripting                  k8pk rm NAME --yes
  TLS issues (one shot)                        k8pk ctx NAME --insecure
  TLS for whole class of clusters              ~/.config/k8pk/config.yaml → insecure_contexts:
  Multi-shell / tmux sessions                  k8pk sessions
  Login / wizard                               k8pk login --wizard
  Diagnose PATH, kubeconfig, shell hooks       k8pk doctor
  Clean old generated files in ~/.local/...    k8pk cleanup
  Advanced: edit one file by hand              k8pk remove-context --from-file PATH ...

  k8pk vs kubie (mental model)
----------------------------
  kubie: namespace as a first-class "layer" on one KUBECONFIG.
  k8pk:  per-terminal isolated kubeconfig + eval exports + optional tmux windows/sessions,
         vault + relogin for OCP/Rancher, rm, doctor, and JSON for scripting.

Rancher (RKE1 / RKE2, same Rancher login API)
---------------------------------------------
  Token:   k8pk login --type rancher --auth token https://rancher.example.com --token $TOKEN
  Userpass: k8pk login --type rancher --auth userpass https://rancher.example.com -u USER -p PASS
  Provider: --rancher-auth-provider local | activedirectory | openldap | freeipa | azuread | auto | ...
  (auto tries common providers; local falls back to AD on 401 where Rancher uses AD-backed users.)
  Vault:   k8pk login --type rancher --auth userpass ... --use-vault  (stores provider for relogin)
  pass:    optional lines rancher_auth_provider: openldap  (or rancher_provider: ...)
  OIDC/SAML-only Rancher: use token auth, not password, unless your IdP exposes v3-public password login.
  Prime (bulk): k8pk rancher pull https://rancher.example.com -u USER -p PASS
                pulls a kubeconfig for every cluster you can access into ~/.kube/rancher/.
                --token / --use-vault / --pattern '<glob>' / --json all supported.

When to use which command
-------------------------
  pick / bare `k8pk`     One short picker, then a shell (or env on non-TTY).
  ctx                    Switch context (history, patterns, tmux).
  ns                     Switch namespace in current k8pk context.
  rm                     Remove a context from the right kubeconfig (preferred).
  remove-context         Same removal logic, but you must pass --from-file explicitly.
  cleanup                Deletes stale files under ~/.local/share/k8pk (not cluster entries).
  sessions               List or jump between k8pk/tmux sessions.
  login                  Add new clusters; use --wizard to start.

Tips
----
  Run `k8pk` from a script with stdout redirected: use `k8pk ctx NAME -o json` or `-o env`.
  `k8pk exec` runs the same session check as `k8pk ctx` (re-login when needed). Use
  `k8pk exec ... --no-session-check` for fast fail if the token is expired.
  Config hooks: stop_ctx runs when leaving a context; start_ctx when entering (eval path).
  After `k8pk config init`, edit the file to set include globs, aliases, tmux, insecure_contexts.
  Set NO_COLOR=1 if terminal colors are unreadable (e.g. `k8pk doctor`).
  Fish: `k8pk ctx` / `k8pk pick` emit fish syntax when FISH_VERSION is set or SHELL is fish.

"#;

#[cfg(test)]
mod tests {
    use super::GUIDE_TEXT;

    #[test]
    fn guide_mentions_core_commands() {
        assert!(GUIDE_TEXT.contains("k8pk rm"));
        assert!(GUIDE_TEXT.contains("kubie"));
        assert!(GUIDE_TEXT.contains("doctor"));
        assert!(GUIDE_TEXT.contains("rancher-auth-provider"));
    }
}
