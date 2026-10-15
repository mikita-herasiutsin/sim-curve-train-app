use std::env;
use std::fs;
use std::process::ExitCode;

use serde::{Deserialize, Serialize};
use syn::visit::Visit;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Entry {
    file: String,
    name: String,
    start_line: u32,
    end_line: u32,
    complexity: u32,
}

fn to_u32(val: usize) -> u32 {
    u32::try_from(val).unwrap_or(u32::MAX)
}

fn fn_start_line(vis: Option<&syn::Visibility>, sig: &syn::Signature) -> u32 {
    let span = match vis {
        Some(syn::Visibility::Public(t)) => t.span,
        Some(syn::Visibility::Restricted(r)) => r.pub_token.span,
        _ => sig
            .constness
            .map(|t| t.span)
            .or_else(|| sig.asyncness.map(|t| t.span))
            .or_else(|| sig.unsafety.map(|t| t.span))
            .unwrap_or(sig.fn_token.span),
    };
    to_u32(span.start().line)
}

fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let mut is_test = attr.path().is_ident("test");
        if attr.path().is_ident("cfg") {
            let _ = attr.parse_nested_meta(|m| {
                if m.path.is_ident("test") {
                    is_test = true;
                }
                Ok(())
            });
        }
        is_test
    })
}

fn self_type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map_or_else(|| "Self".to_string(), |s| s.ident.to_string()),
        syn::Type::Reference(tr) => self_type_name(&tr.elem),
        syn::Type::Group(g) => self_type_name(&g.elem),
        syn::Type::Paren(p) => self_type_name(&p.elem),
        _ => "Self".to_string(),
    }
}

struct ComplexityVisitor {
    complexity: u32,
}

impl<'ast> Visit<'ast> for ComplexityVisitor {
    fn visit_item(&mut self, _node: &'ast syn::Item) {
        // Nested items do not count toward the enclosing function's complexity.
    }

    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        match node {
            syn::Expr::If(_)
            | syn::Expr::While(_)
            | syn::Expr::ForLoop(_)
            | syn::Expr::Loop(_)
            | syn::Expr::Try(_) => self.complexity += 1,
            syn::Expr::Binary(b) if matches!(b.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                self.complexity += 1;
            }
            _ => {}
        }
        syn::visit::visit_expr(self, node);
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        self.complexity += to_u32(node.arms.len().saturating_sub(1));
        syn::visit::visit_expr_match(self, node);
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        if node.guard.is_some() {
            self.complexity += 1;
        }
        syn::visit::visit_arm(self, node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        if node.init.as_ref().is_some_and(|i| i.diverge.is_some()) {
            self.complexity += 1;
        }
        syn::visit::visit_local(self, node);
    }
}

fn compute_complexity(block: &syn::Block) -> u32 {
    let mut visitor = ComplexityVisitor { complexity: 1 };
    visitor.visit_block(block);
    visitor.complexity
}

struct FnCollector<'a> {
    file: &'a str,
    current_impl: Option<String>,
    current_trait: Option<String>,
    entries: Vec<Entry>,
}

impl FnCollector<'_> {
    fn record(
        &mut self,
        name: String,
        vis: Option<&syn::Visibility>,
        sig: &syn::Signature,
        block: &syn::Block,
    ) {
        self.entries.push(Entry {
            file: self.file.to_string(),
            name,
            start_line: fn_start_line(vis, sig),
            end_line: to_u32(block.brace_token.span.close().end().line),
            complexity: compute_complexity(block),
        });
    }

    fn nest_body<F: FnOnce(&mut Self)>(&mut self, f: F) {
        let (prev_impl, prev_trait) = (self.current_impl.take(), self.current_trait.take());
        f(self);
        (self.current_impl, self.current_trait) = (prev_impl, prev_trait);
    }
}

impl<'ast> Visit<'ast> for FnCollector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !has_test_attr(&node.attrs) {
            syn::visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if !has_test_attr(&node.attrs) {
            let prev = self.current_impl.replace(self_type_name(&node.self_ty));
            syn::visit::visit_item_impl(self, node);
            self.current_impl = prev;
        }
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if !has_test_attr(&node.attrs) {
            let prev = self.current_trait.replace(node.ident.to_string());
            syn::visit::visit_item_trait(self, node);
            self.current_trait = prev;
        }
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if has_test_attr(&node.attrs) {
            return;
        }
        self.record(
            node.sig.ident.to_string(),
            Some(&node.vis),
            &node.sig,
            &node.block,
        );
        self.nest_body(|this| syn::visit::visit_item_fn(this, node));
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if has_test_attr(&node.attrs) {
            return;
        }
        let ident = &node.sig.ident;
        let name = self
            .current_impl
            .as_deref()
            .map_or_else(|| ident.to_string(), |ty| format!("{ty}::{ident}"));
        self.record(name, Some(&node.vis), &node.sig, &node.block);
        self.nest_body(|this| syn::visit::visit_impl_item_fn(this, node));
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if has_test_attr(&node.attrs) {
            return;
        }
        if let Some(block) = &node.default {
            let ident = &node.sig.ident;
            let name = self
                .current_trait
                .as_deref()
                .map_or_else(|| ident.to_string(), |tr| format!("{tr}::{ident}"));
            self.record(name, None, &node.sig, block);
            self.nest_body(|this| syn::visit::visit_trait_item_fn(this, node));
        }
    }
}

fn collect_entries(file: &str, parsed: &syn::File) -> Vec<Entry> {
    let mut collector = FnCollector {
        file,
        current_impl: None,
        current_trait: None,
        entries: Vec::new(),
    };
    collector.visit_file(parsed);
    collector.entries
}

fn parse_and_collect(path: &str) -> Result<Vec<Entry>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("complexity: {path}: {e}"))?;
    let file = syn::parse_file(&text).map_err(|e| format!("complexity: {path}: {e}"))?;
    Ok(collect_entries(path, &file))
}

fn main() -> ExitCode {
    let mut all_entries = Vec::new();
    let mut had_error = false;
    for path in env::args().skip(1) {
        match parse_and_collect(&path) {
            Ok(entries) => all_entries.extend(entries),
            Err(err) => {
                eprintln!("{err}");
                had_error = true;
            }
        }
    }
    all_entries.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.start_line.cmp(&b.start_line))
    });
    let json = serde_json::to_string_pretty(&all_entries).expect("serialize entries");
    println!("{json}");
    if had_error {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;

    fn analyze_str(file: &str, src: &str) -> Vec<Entry> {
        let parsed = syn::parse_file(src).expect("parse valid source");
        let mut entries = collect_entries(file, &parsed);
        entries.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then_with(|| a.start_line.cmp(&b.start_line))
        });
        entries
    }

    #[test]
    fn straight_line_fn_gives_one() {
        let src = "fn straight() { let a = 1; let b = 2; }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].complexity, 1);
    }

    #[test]
    fn if_else_if_else_gives_three() {
        let src = "fn branching(x: i32) { if x > 0 {} else if x < 0 {} else {} }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].complexity, 3);
    }

    #[test]
    fn match_with_four_arms_and_one_guard_gives_five() {
        let src = "fn matching(x: i32) { match x { 1 if true => {} 2 => {} 3 => {} _ => {} } }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].complexity, 5);
    }

    #[test]
    fn binary_and_or_gives_three() {
        let src = "fn logic(a: bool, b: bool, c: bool) -> bool { a && b || c }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].complexity, 3);
    }

    #[test]
    fn two_try_operators_give_three() {
        let src = "fn questions() -> Result<(), ()> { foo()?; bar()?; Ok(()) }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].complexity, 3);
    }

    #[test]
    fn while_let_for_and_loop_each_add_one() {
        let src_while = "fn test_while() { while let Some(_) = None {} }";
        let src_for = "fn test_for() { for _ in 0..1 {} }";
        let src_loop = "fn test_loop() { loop { break; } }";

        let entries_while = analyze_str("test.rs", src_while);
        let entries_for = analyze_str("test.rs", src_for);
        let entries_loop = analyze_str("test.rs", src_loop);

        assert_eq!(entries_while[0].complexity, 2);
        assert_eq!(entries_for[0].complexity, 2);
        assert_eq!(entries_loop[0].complexity, 2);
    }

    #[test]
    fn let_else_gives_two() {
        let src = "fn let_else(y: Option<i32>) { let Some(x) = y else { return; }; }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].complexity, 2);
    }

    #[test]
    fn closure_with_if_adds_to_enclosing_fn() {
        let src = "fn with_closure(x: bool) { let c = || { if x {} }; }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "with_closure");
        assert_eq!(entries[0].complexity, 2);
    }

    #[test]
    fn nested_fn_with_if_reported_separately() {
        let src = "fn outer() { fn inner(x: bool) { if x {} } }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "outer");
        assert_eq!(entries[0].complexity, 1);
        assert_eq!(entries[1].name, "inner");
        assert_eq!(entries[1].complexity, 2);
    }

    #[test]
    fn impl_method_and_trait_default_method_names() {
        let src = "impl<T> Foo<T> { fn bar() {} }\ntrait Tr { fn baz() {} }";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "Foo::bar");
        assert_eq!(entries[1].name, "Tr::baz");
    }

    #[test]
    fn skip_cfg_test_module_and_test_fn() {
        let src = "#[cfg(test)] mod tests { fn helper() { if x {} } }\n#[test] fn t() {}";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries, &[]);
    }

    #[test]
    fn start_and_end_line_span() {
        let src = "/// doc comment\n#[inline]\nfn multi_line() {\n    let _ = 1;\n}";
        let entries = analyze_str("test.rs", src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].start_line, 3);
        assert_eq!(entries[0].end_line, 5);
    }

    #[test]
    fn audio_map_end_to_end() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.join("../..");
        let output = Command::new(env!("CARGO"))
            .args([
                "run",
                "-q",
                "-p",
                "complexity",
                "--",
                "crates/core/src/audio_map.rs",
            ])
            .current_dir(&workspace_root)
            .output()
            .expect("run complexity CLI");
        assert!(output.status.success());

        let entries: Vec<Entry> =
            serde_json::from_slice(&output.stdout).expect("parse JSON output");
        let pulse_entry = entries.iter().find(|e| e.name == "pulse_rate_hz");
        assert!(pulse_entry.is_some());
        let pulse = pulse_entry.unwrap();
        assert_eq!(pulse.start_line, 25);
        assert_eq!(pulse.end_line, 34);
        assert_eq!(pulse.complexity, 3);
    }
}
