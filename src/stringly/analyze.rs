use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::ToTokens;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    BinOp, Expr, ExprCall, ExprMatch, ExprMethodCall, ExprStruct, FnArg, GenericArgument,
    ImplItemFn, ItemFn, ItemImpl, ItemMod, ItemStruct, ItemTrait, Lit, Member, Meta, Pat,
    PathArguments, ReturnType, Signature, Token, TraitItemFn, Type, UseTree, Visibility,
    punctuated::Punctuated,
};

use crate::project::ProjectData;

use super::dependencies::{DependencyCatalog, normalize_crate_name};
use super::model::{
    StringSiteKind, StringlyCaveat, StringlyCaveatKind, StringlyConfig, StringlyEvidence,
    StringlyEvidenceKind, StringlyFinding, StringlyLiteralStats, StringlyReport,
    StringlySuppressionCounts, SuggestedType, TypeOrigin,
};

/// Analyse the cached ASTs in `project` and return ranked, evidence-backed
/// opportunities to replace `String`/`&str` declarations with stronger types.
pub fn detect_stringly(project: &ProjectData, config: &StringlyConfig) -> StringlyReport {
    let min_confidence = if config.min_confidence.is_finite() {
        config.min_confidence.clamp(0.0, 1.0)
    } else {
        StringlyConfig::default().min_confidence
    };
    let catalog = DependencyCatalog::discover(&config.project_roots, &project.rust_files);
    let local_types = LocalTypeCatalog::from_project(project);
    // CLI-loaded projects always hit the eager AST cache. Deserialized
    // `ProjectData` deliberately omits that cache, so parse each missing file
    // once and reuse the fallback in both detector passes.
    let fallback_asts: HashMap<String, syn::File> = project
        .rust_files
        .iter()
        .filter(|file| project.parsed_file(file).is_none())
        .filter_map(|file| {
            let source = fs::read_to_string(file).ok()?;
            let syntax = syn::parse_file(&source).ok()?;
            Some((
                crate::normalize_path_separators(&file.to_string_lossy()),
                syntax,
            ))
        })
        .collect();
    let mut sites = Vec::new();
    let mut imports_by_file = HashMap::new();
    let mut visited_files = HashSet::new();

    for file in &project.rust_files {
        let file_path = crate::normalize_path_separators(&file.to_string_lossy());
        if !visited_files.insert(file_path.clone()) {
            continue;
        }
        let Some(syntax) = project
            .parsed_file(file)
            .or_else(|| fallback_asts.get(&file_path))
        else {
            continue;
        };
        imports_by_file.insert(file_path.clone(), collect_imports(syntax));
        let provenance = classify_source_file(file);
        let mut collector = SiteCollector::new(&file_path, provenance, &mut sites);
        collector.visit_file(syntax);
    }

    let scanned_string_sites = sites
        .iter()
        .filter(|site| config.include_tests || !site.is_test)
        .count();
    if !config.include_tests {
        // Test declarations must not participate in resolution either: a test
        // helper with the same function name can otherwise make a production
        // call target ambiguous and silently discard real evidence.
        sites.retain(|site| !site.is_test);
    }
    let index = SiteIndex::build(&sites);
    let mut visited_files = HashSet::new();
    for file in &project.rust_files {
        let file_path = crate::normalize_path_separators(&file.to_string_lossy());
        if !visited_files.insert(file_path.clone()) {
            continue;
        }
        if !config.include_tests && is_test_path(&file_path) {
            continue;
        }
        let Some(syntax) = project
            .parsed_file(file)
            .or_else(|| fallback_asts.get(&file_path))
        else {
            continue;
        };
        let mut visitor =
            EvidenceVisitor::new(&file_path, &index, &mut sites, config.include_tests);
        visitor.visit_file(syntax);
    }
    propagate_diagnostic_sinks(&mut sites);
    annotate_shared_vocabularies(&mut sites, config.include_tests);

    let mut findings = Vec::new();
    let mut suppressed_findings = 0usize;
    let mut suppression_counts = StringlySuppressionCounts::default();
    for site in sites
        .into_iter()
        .filter(|site| config.include_tests || !site.is_test)
    {
        let Some(imports) = imports_by_file.get(&site.file_path) else {
            continue;
        };
        let Some(finding) = finalise_site(site, &catalog, imports, &local_types, min_confidence)
        else {
            continue;
        };
        if finding.is_suppressed_by_default() {
            suppressed_findings += 1;
            for caveat in &finding.caveats {
                if caveat.suppresses_by_default {
                    suppression_counts.record(caveat.kind);
                }
            }
            if !config.include_suppressed {
                continue;
            }
        }
        findings.push(finding);
    }

    findings.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| left.file_path.cmp(&right.file_path))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.name.cmp(&right.name))
    });

    StringlyReport {
        scanned_string_sites,
        suppressed_findings,
        suppression_counts,
        findings,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileProvenance {
    Project,
    Generated,
    Vendored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallableKind {
    Free,
    Method,
}

#[derive(Debug, Clone)]
struct Site {
    file_path: String,
    line: usize,
    name: String,
    owner: String,
    site_kind: StringSiteKind,
    declared_type: String,
    scope_id: Option<String>,
    function_name: Option<String>,
    parameter_position: Option<usize>,
    callable_kind: Option<CallableKind>,
    callable_owner: Option<String>,
    field_owner: Option<String>,
    return_type_names: BTreeSet<String>,
    trait_contract: bool,
    provenance: FileProvenance,
    public_api: bool,
    external_representation: bool,
    is_test: bool,
    evidence: Vec<StringlyEvidence>,
    literals: BTreeSet<String>,
    literal_observations: usize,
    literal_comparisons: usize,
    literal_matches: usize,
    literal_constructions: usize,
    literal_call_sites: usize,
    matched_literals: BTreeSet<String>,
    supplied_literals: BTreeSet<String>,
    match_arm_targets: Vec<BTreeSet<String>>,
    conversion_targets: BTreeSet<String>,
    use_sites: BTreeSet<(usize, usize)>,
    conversion_use_sites: BTreeSet<(usize, usize)>,
    diagnostic_use_sites: BTreeSet<(usize, usize)>,
    forwarded_uses: Vec<ForwardedUse>,
    saw_literal_match: bool,
    saw_rejecting_match: bool,
    shared_vocabulary: Option<SharedVocabulary>,
}

#[derive(Debug, Clone)]
struct SharedVocabulary {
    type_name: String,
    declaration_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForwardedUse {
    location: (usize, usize),
    target_site: usize,
}

impl Site {
    fn add_evidence(
        &mut self,
        kind: StringlyEvidenceKind,
        message: String,
        file_path: &str,
        line: usize,
    ) {
        let evidence = StringlyEvidence {
            kind,
            message,
            file_path: file_path.to_string(),
            line,
        };
        if !self.evidence.contains(&evidence) {
            self.evidence.push(evidence);
        }
    }

    fn add_literal(
        &mut self,
        literal: String,
        kind: StringlyEvidenceKind,
        message: String,
        file_path: &str,
        line: usize,
    ) {
        self.literal_observations += 1;
        match kind {
            StringlyEvidenceKind::LiteralComparison => self.literal_comparisons += 1,
            StringlyEvidenceKind::LiteralMatch => self.literal_matches += 1,
            StringlyEvidenceKind::LiteralConstruction => self.literal_constructions += 1,
            StringlyEvidenceKind::LiteralCallSite => self.literal_call_sites += 1,
            StringlyEvidenceKind::ExplicitConversion
            | StringlyEvidenceKind::SharedVocabulary
            | StringlyEvidenceKind::StructuredLiteral
            | StringlyEvidenceKind::SemanticName => {}
        }
        self.literals.insert(literal.clone());
        if kind == StringlyEvidenceKind::LiteralMatch {
            self.saw_literal_match = true;
            self.matched_literals.insert(literal);
        } else if matches!(
            kind,
            StringlyEvidenceKind::LiteralComparison
                | StringlyEvidenceKind::LiteralConstruction
                | StringlyEvidenceKind::LiteralCallSite
        ) {
            self.supplied_literals.insert(literal);
        }
        self.add_evidence(kind, message, file_path, line);
    }
}

struct SiteCollector<'a> {
    file_path: &'a str,
    sites: &'a mut Vec<Site>,
    provenance: FileProvenance,
    current_scope: Option<String>,
    current_owner: Option<String>,
    current_function: Option<String>,
    current_trait_contract: bool,
    inherited_test: bool,
}

struct CallableContext {
    owner: String,
    kind: CallableKind,
    type_owner: Option<String>,
    public_api: bool,
    trait_contract: bool,
}

impl<'a> SiteCollector<'a> {
    fn new(file_path: &'a str, provenance: FileProvenance, sites: &'a mut Vec<Site>) -> Self {
        Self {
            file_path,
            sites,
            provenance,
            current_scope: None,
            current_owner: None,
            current_function: None,
            current_trait_contract: false,
            inherited_test: is_test_path(file_path),
        }
    }

    fn collect_parameters(
        &mut self,
        signature: &Signature,
        scope_id: &str,
        context: &CallableContext,
        is_test: bool,
    ) {
        let return_type_names = return_type_names(signature, context.type_owner.as_deref());
        let mut position = 0usize;
        for argument in &signature.inputs {
            let FnArg::Typed(typed) = argument else {
                continue;
            };
            if let Some(name) = simple_pat_name(&typed.pat)
                && is_string_type(&typed.ty)
            {
                self.sites.push(Site {
                    file_path: self.file_path.to_string(),
                    line: typed.span().start().line,
                    name,
                    owner: context.owner.clone(),
                    site_kind: StringSiteKind::FunctionParameter,
                    declared_type: render_tokens(&typed.ty),
                    scope_id: Some(scope_id.to_string()),
                    function_name: Some(signature.ident.to_string()),
                    parameter_position: Some(position),
                    callable_kind: Some(context.kind),
                    callable_owner: context.type_owner.clone(),
                    field_owner: None,
                    return_type_names: return_type_names.clone(),
                    trait_contract: context.trait_contract,
                    provenance: self.provenance,
                    public_api: context.public_api,
                    external_representation: false,
                    is_test,
                    evidence: Vec::new(),
                    literals: BTreeSet::new(),
                    literal_observations: 0,
                    literal_comparisons: 0,
                    literal_matches: 0,
                    literal_constructions: 0,
                    literal_call_sites: 0,
                    matched_literals: BTreeSet::new(),
                    supplied_literals: BTreeSet::new(),
                    match_arm_targets: Vec::new(),
                    conversion_targets: BTreeSet::new(),
                    use_sites: BTreeSet::new(),
                    conversion_use_sites: BTreeSet::new(),
                    diagnostic_use_sites: BTreeSet::new(),
                    forwarded_uses: Vec::new(),
                    saw_literal_match: false,
                    saw_rejecting_match: false,
                    shared_vocabulary: None,
                });
            }
            position += 1;
        }
    }

    fn visit_function_body(
        &mut self,
        signature: &Signature,
        block: &syn::Block,
        context: CallableContext,
        attrs: &[syn::Attribute],
    ) {
        let scope_id = scope_id(self.file_path, signature.ident.span());
        let is_test = self.inherited_test || has_test_attr(attrs);
        self.collect_parameters(signature, &scope_id, &context, is_test);

        let old_scope = self.current_scope.replace(scope_id);
        let old_owner = self.current_owner.replace(context.owner);
        let old_function = self.current_function.replace(signature.ident.to_string());
        let old_test = self.inherited_test;
        self.inherited_test = is_test;
        self.visit_block(block);
        self.inherited_test = old_test;
        self.current_function = old_function;
        self.current_owner = old_owner;
        self.current_scope = old_scope;
    }
}

impl<'ast> Visit<'ast> for SiteCollector<'_> {
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        let old_test = self.inherited_test;
        self.inherited_test =
            old_test || is_test_module_name(&item.ident.to_string()) || has_test_attr(&item.attrs);
        if let Some((_, items)) = &item.content {
            for nested in items {
                self.visit_item(nested);
            }
        }
        self.inherited_test = old_test;
    }

    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        let owner = item.ident.to_string();
        let is_test = self.inherited_test || has_test_attr(&item.attrs);
        let struct_has_external_representation = has_external_representation_attr(&item.attrs);
        for field in &item.fields {
            let Some(ident) = &field.ident else {
                continue;
            };
            if !is_string_type(&field.ty) {
                continue;
            }
            self.sites.push(Site {
                file_path: self.file_path.to_string(),
                line: field.span().start().line,
                name: ident.to_string(),
                owner: owner.clone(),
                site_kind: StringSiteKind::StructField,
                declared_type: render_tokens(&field.ty),
                scope_id: None,
                function_name: None,
                parameter_position: None,
                callable_kind: None,
                callable_owner: None,
                field_owner: Some(owner.clone()),
                return_type_names: BTreeSet::new(),
                trait_contract: false,
                provenance: self.provenance,
                public_api: is_public(&field.vis),
                external_representation: struct_has_external_representation
                    || has_external_representation_attr(&field.attrs),
                is_test,
                evidence: Vec::new(),
                literals: BTreeSet::new(),
                literal_observations: 0,
                literal_comparisons: 0,
                literal_matches: 0,
                literal_constructions: 0,
                literal_call_sites: 0,
                matched_literals: BTreeSet::new(),
                supplied_literals: BTreeSet::new(),
                match_arm_targets: Vec::new(),
                conversion_targets: BTreeSet::new(),
                use_sites: BTreeSet::new(),
                conversion_use_sites: BTreeSet::new(),
                diagnostic_use_sites: BTreeSet::new(),
                forwarded_uses: Vec::new(),
                saw_literal_match: false,
                saw_rejecting_match: false,
                shared_vocabulary: None,
            });
        }
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.visit_function_body(
            &item.sig,
            &item.block,
            CallableContext {
                owner: item.sig.ident.to_string(),
                kind: CallableKind::Free,
                type_owner: None,
                public_api: is_public(&item.vis),
                trait_contract: false,
            },
            &item.attrs,
        );
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        let old_owner = self.current_owner.replace(type_head(&item.self_ty));
        let old_trait_contract = self.current_trait_contract;
        self.current_trait_contract = item.trait_.is_some();
        for nested in &item.items {
            self.visit_impl_item(nested);
        }
        self.current_trait_contract = old_trait_contract;
        self.current_owner = old_owner;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        let type_owner = self
            .current_owner
            .clone()
            .unwrap_or_else(|| "<impl>".to_string());
        let owner = format!("{}::{}", type_owner, item.sig.ident);
        self.visit_function_body(
            &item.sig,
            &item.block,
            CallableContext {
                owner,
                kind: CallableKind::Method,
                type_owner: Some(type_owner),
                public_api: is_public(&item.vis),
                trait_contract: self.current_trait_contract,
            },
            &item.attrs,
        );
    }

    fn visit_item_trait(&mut self, item: &'ast ItemTrait) {
        let old_owner = self.current_owner.replace(item.ident.to_string());
        let old_trait_contract = self.current_trait_contract;
        self.current_trait_contract = true;
        for nested in &item.items {
            self.visit_trait_item(nested);
        }
        self.current_trait_contract = old_trait_contract;
        self.current_owner = old_owner;
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        let trait_owner = self
            .current_owner
            .clone()
            .unwrap_or_else(|| "<trait>".to_string());
        let owner = format!("{}::{}", trait_owner, item.sig.ident);
        let scope_id = scope_id(self.file_path, item.sig.ident.span());
        let is_test = self.inherited_test || has_test_attr(&item.attrs);
        self.collect_parameters(
            &item.sig,
            &scope_id,
            &CallableContext {
                owner: owner.clone(),
                kind: CallableKind::Method,
                type_owner: Some(trait_owner),
                public_api: true,
                trait_contract: true,
            },
            is_test,
        );
        if let Some(block) = &item.default {
            let old_scope = self.current_scope.replace(scope_id);
            let old_owner = self.current_owner.replace(owner);
            let old_function = self.current_function.replace(item.sig.ident.to_string());
            let old_test = self.inherited_test;
            self.inherited_test = is_test;
            self.visit_block(block);
            self.inherited_test = old_test;
            self.current_function = old_function;
            self.current_owner = old_owner;
            self.current_scope = old_scope;
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Some(scope_id) = &self.current_scope
            && let Pat::Type(typed) = &local.pat
            && let Some(name) = simple_pat_name(&typed.pat)
            && is_string_type(&typed.ty)
        {
            self.sites.push(Site {
                file_path: self.file_path.to_string(),
                line: local.span().start().line,
                name,
                owner: self
                    .current_owner
                    .clone()
                    .unwrap_or_else(|| "<local>".to_string()),
                site_kind: StringSiteKind::LocalBinding,
                declared_type: render_tokens(&typed.ty),
                scope_id: Some(scope_id.clone()),
                function_name: self.current_function.clone(),
                parameter_position: None,
                callable_kind: None,
                callable_owner: None,
                field_owner: None,
                return_type_names: BTreeSet::new(),
                trait_contract: false,
                provenance: self.provenance,
                public_api: false,
                external_representation: false,
                is_test: self.inherited_test,
                evidence: Vec::new(),
                literals: BTreeSet::new(),
                literal_observations: 0,
                literal_comparisons: 0,
                literal_matches: 0,
                literal_constructions: 0,
                literal_call_sites: 0,
                matched_literals: BTreeSet::new(),
                supplied_literals: BTreeSet::new(),
                match_arm_targets: Vec::new(),
                conversion_targets: BTreeSet::new(),
                use_sites: BTreeSet::new(),
                conversion_use_sites: BTreeSet::new(),
                diagnostic_use_sites: BTreeSet::new(),
                forwarded_uses: Vec::new(),
                saw_literal_match: false,
                saw_rejecting_match: false,
                shared_vocabulary: None,
            });
        }
        syn::visit::visit_local(self, local);
    }
}

#[derive(Default)]
struct SiteIndex {
    scoped: HashMap<(String, String), Vec<usize>>,
    fields_by_owner: HashMap<(String, String), Vec<usize>>,
    free_call_parameters: HashMap<(String, usize), Vec<usize>>,
    method_call_parameters: HashMap<(String, usize), Vec<usize>>,
    associated_call_parameters: HashMap<(String, String, usize), Vec<usize>>,
}

impl SiteIndex {
    fn build(sites: &[Site]) -> Self {
        let mut index = Self::default();
        for (position, site) in sites.iter().enumerate() {
            if let Some(scope_id) = &site.scope_id {
                index
                    .scoped
                    .entry((scope_id.clone(), site.name.clone()))
                    .or_default()
                    .push(position);
            }
            if let Some(owner) = &site.field_owner {
                index
                    .fields_by_owner
                    .entry((owner.clone(), site.name.clone()))
                    .or_default()
                    .push(position);
            }
            if let (Some(function), Some(parameter_position), Some(callable_kind)) = (
                &site.function_name,
                site.parameter_position,
                site.callable_kind,
            ) {
                match callable_kind {
                    CallableKind::Free => index
                        .free_call_parameters
                        .entry((function.clone(), parameter_position))
                        .or_default()
                        .push(position),
                    CallableKind::Method => {
                        index
                            .method_call_parameters
                            .entry((function.clone(), parameter_position))
                            .or_default()
                            .push(position);
                        if let Some(owner) = &site.callable_owner {
                            index
                                .associated_call_parameters
                                .entry((owner.clone(), function.clone(), parameter_position))
                                .or_default()
                                .push(position);
                        }
                    }
                }
            }
        }
        for positions in index.scoped.values_mut() {
            positions.sort_by_key(|position| sites[*position].line);
        }
        index
    }
}

struct EvidenceVisitor<'a> {
    file_path: &'a str,
    index: &'a SiteIndex,
    sites: &'a mut [Site],
    current_scope: Option<String>,
    current_impl: Option<String>,
    current_return_type_names: BTreeSet<String>,
    binding_scopes: Vec<HashMap<String, String>>,
    include_tests: bool,
    inherited_test: bool,
}

impl<'a> EvidenceVisitor<'a> {
    fn new(
        file_path: &'a str,
        index: &'a SiteIndex,
        sites: &'a mut [Site],
        include_tests: bool,
    ) -> Self {
        Self {
            file_path,
            index,
            sites,
            current_scope: None,
            current_impl: None,
            current_return_type_names: BTreeSet::new(),
            binding_scopes: Vec::new(),
            include_tests,
            inherited_test: is_test_path(file_path),
        }
    }

    fn resolve_expr(&self, expression: &Expr) -> Option<usize> {
        match expression {
            Expr::Reference(reference) => self.resolve_expr(&reference.expr),
            Expr::Paren(paren) => self.resolve_expr(&paren.expr),
            Expr::Group(group) => self.resolve_expr(&group.expr),
            Expr::Try(try_expr) => self.resolve_expr(&try_expr.expr),
            Expr::Await(await_expr) => self.resolve_expr(&await_expr.base),
            Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                self.resolve_expr(&unary.expr)
            }
            Expr::MethodCall(call)
                if call.args.is_empty()
                    && matches!(
                        call.method.to_string().as_str(),
                        "as_str" | "as_ref" | "as_deref" | "borrow" | "trim"
                    ) =>
            {
                self.resolve_expr(&call.receiver)
            }
            Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => {
                let name = path.path.segments[0].ident.to_string();
                self.resolve_scoped_name(&name, expression.span().start().line)
            }
            Expr::Field(field) => {
                let Member::Named(member) = &field.member else {
                    return None;
                };
                let name = member.to_string();
                if is_self_expr(&field.base)
                    && let Some(owner) = &self.current_impl
                    && let Some(candidates) = self
                        .index
                        .fields_by_owner
                        .get(&(owner.clone(), name.clone()))
                {
                    return unique_in_file(candidates, self.sites, self.file_path);
                }
                let owner = self.resolve_binding_owner(&field.base)?;
                self.index
                    .fields_by_owner
                    .get(&(owner, name))
                    .and_then(|candidates| unique_in_file(candidates, self.sites, self.file_path))
            }
            _ => None,
        }
    }

    fn resolve_scoped_name(&self, name: &str, line: usize) -> Option<usize> {
        let scope = self.current_scope.as_ref()?;
        let candidates = self.index.scoped.get(&(scope.clone(), name.to_string()))?;
        candidates
            .iter()
            .copied()
            .filter(|position| self.sites[*position].line <= line)
            .max_by_key(|position| self.sites[*position].line)
    }

    fn resolve_binding_owner(&self, expression: &Expr) -> Option<String> {
        let name = match expression {
            Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => {
                path.path.segments[0].ident.to_string()
            }
            Expr::Reference(reference) => return self.resolve_binding_owner(&reference.expr),
            Expr::Paren(paren) => return self.resolve_binding_owner(&paren.expr),
            Expr::Group(group) => return self.resolve_binding_owner(&group.expr),
            Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                return self.resolve_binding_owner(&unary.expr);
            }
            _ => return None,
        };
        self.binding_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&name).cloned())
    }

    fn push_signature_bindings(&mut self, signature: &Signature) {
        let mut bindings = HashMap::new();
        for argument in &signature.inputs {
            let FnArg::Typed(typed) = argument else {
                continue;
            };
            if let Some(name) = simple_pat_name(&typed.pat) {
                bindings.insert(name, type_head(&typed.ty));
            }
        }
        self.binding_scopes.push(bindings);
    }

    fn record_local_binding(&mut self, local: &syn::Local) {
        let binding = match &local.pat {
            Pat::Type(typed) => {
                simple_pat_name(&typed.pat).map(|name| (name, type_head(&typed.ty)))
            }
            pattern => {
                let name = simple_pat_name(pattern);
                let owner = local
                    .init
                    .as_ref()
                    .and_then(|initializer| struct_expression_owner(&initializer.expr));
                name.zip(owner)
            }
        };
        if let Some((name, owner)) = binding
            && let Some(scope) = self.binding_scopes.last_mut()
        {
            scope.insert(name, owner);
        }
    }

    fn record_literal(
        &mut self,
        site_position: usize,
        literal: String,
        kind: StringlyEvidenceKind,
        line: usize,
        context: &str,
    ) {
        let literal = literal_preview(&literal);
        let message = format!("{} string literal {:?}", context, literal);
        self.sites[site_position].add_literal(literal, kind, message, self.file_path, line);
    }

    fn record_conversion(&mut self, site_position: usize, target: String, expression: &Expr) {
        if is_textual_target(&target) {
            return;
        }
        let line = expression.span().start().line;
        self.sites[site_position]
            .conversion_targets
            .insert(target.clone());
        self.sites[site_position]
            .conversion_use_sites
            .insert(use_site(expression));
        self.sites[site_position].add_evidence(
            StringlyEvidenceKind::ExplicitConversion,
            format!("converted directly into {}", target),
            self.file_path,
            line,
        );
    }

    fn record_diagnostic_use(&mut self, expression: &Expr) {
        if let Some(site) = self.resolve_expr(expression) {
            self.sites[site]
                .diagnostic_use_sites
                .insert(use_site(expression));
        }
    }

    fn record_call_literals<'b>(
        &mut self,
        candidates_for_position: impl Fn(usize) -> Option<&'b Vec<usize>>,
        arguments: impl Iterator<Item = &'b Expr>,
    ) {
        for (argument_position, argument) in arguments.enumerate() {
            let Some(literal) = extract_string_literal(argument) else {
                continue;
            };
            let Some(candidates) = candidates_for_position(argument_position) else {
                continue;
            };
            if candidates.len() != 1 {
                continue;
            }
            self.record_literal(
                candidates[0],
                literal,
                StringlyEvidenceKind::LiteralCallSite,
                argument.span().start().line,
                "called with",
            );
        }
    }

    fn record_forwarded_arguments<'b>(
        &mut self,
        candidates_for_position: impl Fn(usize) -> Option<&'b Vec<usize>>,
        arguments: impl Iterator<Item = &'b Expr>,
    ) {
        for (argument_position, argument) in arguments.enumerate() {
            let Some(source_site) = self.resolve_expr(argument) else {
                continue;
            };
            let Some(candidates) = candidates_for_position(argument_position) else {
                continue;
            };
            if candidates.len() != 1 || candidates[0] == source_site {
                continue;
            }
            let forwarded = ForwardedUse {
                location: use_site(argument),
                target_site: candidates[0],
            };
            if !self.sites[source_site].forwarded_uses.contains(&forwarded) {
                self.sites[source_site].forwarded_uses.push(forwarded);
            }
        }
    }

    fn visit_function(&mut self, signature: &Signature, block: &syn::Block) {
        let old_scope = self
            .current_scope
            .replace(scope_id(self.file_path, signature.ident.span()));
        let old_return_type_names = std::mem::replace(
            &mut self.current_return_type_names,
            return_type_names(signature, self.current_impl.as_deref()),
        );
        let old_binding_scopes = std::mem::take(&mut self.binding_scopes);
        self.push_signature_bindings(signature);
        syn::visit::visit_block(self, block);
        self.binding_scopes.pop();
        self.binding_scopes = old_binding_scopes;
        self.current_return_type_names = old_return_type_names;
        self.current_scope = old_scope;
    }
}

impl<'ast> Visit<'ast> for EvidenceVisitor<'_> {
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        let old_test = self.inherited_test;
        self.inherited_test =
            old_test || is_test_module_name(&item.ident.to_string()) || has_test_attr(&item.attrs);
        if (self.include_tests || !self.inherited_test)
            && let Some((_, items)) = &item.content
        {
            for nested in items {
                self.visit_item(nested);
            }
        }
        self.inherited_test = old_test;
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if !self.include_tests && (self.inherited_test || has_test_attr(&item.attrs)) {
            return;
        }
        self.visit_function(&item.sig, &item.block);
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        let old_test = self.inherited_test;
        self.inherited_test = old_test || has_test_attr(&item.attrs);
        if !self.include_tests && self.inherited_test {
            self.inherited_test = old_test;
            return;
        }
        let old_impl = self.current_impl.replace(type_head(&item.self_ty));
        for nested in &item.items {
            self.visit_impl_item(nested);
        }
        self.current_impl = old_impl;
        self.inherited_test = old_test;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        if !self.include_tests && (self.inherited_test || has_test_attr(&item.attrs)) {
            return;
        }
        self.visit_function(&item.sig, &item.block);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        if !self.include_tests && (self.inherited_test || has_test_attr(&item.attrs)) {
            return;
        }
        if let Some(block) = &item.default {
            self.visit_function(&item.sig, block);
        }
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.binding_scopes.push(HashMap::new());
        syn::visit::visit_block(self, block);
        self.binding_scopes.pop();
    }

    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        if matches!(binary.op, BinOp::Eq(_) | BinOp::Ne(_)) {
            let line = binary.span().start().line;
            if let (Some(site), Some(literal)) = (
                self.resolve_expr(&binary.left),
                extract_string_literal(&binary.right),
            ) {
                self.record_literal(
                    site,
                    literal,
                    StringlyEvidenceKind::LiteralComparison,
                    line,
                    "compared against",
                );
            } else if let (Some(literal), Some(site)) = (
                extract_string_literal(&binary.left),
                self.resolve_expr(&binary.right),
            ) {
                self.record_literal(
                    site,
                    literal,
                    StringlyEvidenceKind::LiteralComparison,
                    line,
                    "compared against",
                );
            }
        }
        syn::visit::visit_expr_binary(self, binary);
    }

    fn visit_expr_match(&mut self, match_expr: &'ast ExprMatch) {
        if let Some(site) = self.resolve_expr(&match_expr.expr) {
            let has_fallback = match_expr
                .arms
                .iter()
                .any(|arm| arm.guard.is_none() && is_catch_all_pattern(&arm.pat));
            let rejecting_fallback = match_expr
                .arms
                .iter()
                .filter(|arm| arm.guard.is_none() && is_catch_all_pattern(&arm.pat))
                .any(|arm| is_rejecting_expr(&arm.body));
            if rejecting_fallback {
                self.sites[site].saw_rejecting_match = true;
                self.sites[site].add_evidence(
                    StringlyEvidenceKind::LiteralMatch,
                    "unrecognized values are rejected by the match fallback".to_string(),
                    self.file_path,
                    match_expr.span().start().line,
                );
            } else if has_fallback {
                self.sites[site].add_evidence(
                    StringlyEvidenceKind::LiteralMatch,
                    "match accepts other values; preserve the fallback with an `Unknown(String)`-style variant"
                        .to_string(),
                    self.file_path,
                    match_expr.span().start().line,
                );
            }
            for arm in &match_expr.arms {
                let literals = string_literals_in_pattern(&arm.pat);
                if !literals.is_empty() {
                    self.sites[site]
                        .match_arm_targets
                        .push(returned_nominal_types(
                            &arm.body,
                            self.current_impl.as_deref(),
                            &self.current_return_type_names,
                        ));
                }
                for literal in literals {
                    self.record_literal(
                        site,
                        literal,
                        StringlyEvidenceKind::LiteralMatch,
                        arm.pat.span().start().line,
                        "matched against",
                    );
                }
            }
        }
        syn::visit::visit_expr_match(self, match_expr);
    }

    fn visit_expr_struct(&mut self, struct_expr: &'ast ExprStruct) {
        let owner = struct_expr
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        if let Some(owner) = owner {
            for field in &struct_expr.fields {
                let Member::Named(member) = &field.member else {
                    continue;
                };
                let Some(literal) = extract_string_literal(&field.expr) else {
                    continue;
                };
                let Some(candidates) = self
                    .index
                    .fields_by_owner
                    .get(&(owner.clone(), member.to_string()))
                else {
                    continue;
                };
                if let Some(site) = unique_in_file(candidates, self.sites, self.file_path) {
                    self.record_literal(
                        site,
                        literal,
                        StringlyEvidenceKind::LiteralConstruction,
                        field.span().start().line,
                        "constructed with",
                    );
                }
            }
        }
        syn::visit::visit_expr_struct(self, struct_expr);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        let index = self.index;
        self.record_call_literals(
            |position| {
                index
                    .method_call_parameters
                    .get(&(method.clone(), position))
            },
            call.args.iter(),
        );
        self.record_forwarded_arguments(
            |position| {
                index
                    .method_call_parameters
                    .get(&(method.clone(), position))
            },
            call.args.iter(),
        );

        if method == "parse"
            && let Some(site) = self.resolve_expr(&call.receiver)
            && let Some(target) = method_turbofish_type(call)
        {
            self.record_conversion(site, target, &call.receiver);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(function) = &*call.func
            && let Some(last) = function.path.segments.last()
        {
            let leaf = last.ident.to_string();
            let explicit_owner = associated_path_owner(&function.path).map(|owner| {
                if owner == "Self" {
                    self.current_impl.clone().unwrap_or(owner)
                } else {
                    owner
                }
            });
            let index = self.index;
            if let Some(owner) = explicit_owner {
                self.record_call_literals(
                    |position| {
                        index.associated_call_parameters.get(&(
                            owner.clone(),
                            leaf.clone(),
                            position,
                        ))
                    },
                    call.args.iter(),
                );
                self.record_forwarded_arguments(
                    |position| {
                        index.associated_call_parameters.get(&(
                            owner.clone(),
                            leaf.clone(),
                            position,
                        ))
                    },
                    call.args.iter(),
                );
            } else {
                self.record_call_literals(
                    |position| index.free_call_parameters.get(&(leaf.clone(), position)),
                    call.args.iter(),
                );
                self.record_forwarded_arguments(
                    |position| index.free_call_parameters.get(&(leaf.clone(), position)),
                    call.args.iter(),
                );
            }

            if let Some(first_argument) = call.args.first()
                && let Some(site) = self.resolve_expr(first_argument)
                && let Some(target) = conversion_target_from_path(&function.path)
            {
                self.record_conversion(site, target, first_argument);
            }

            if is_diagnostic_constructor_path(&function.path) {
                for argument in &call.args {
                    self.record_diagnostic_use(argument);
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Pat::Type(typed) = &local.pat
            && let Some(initializer) = &local.init
        {
            let expression = peel_control_flow(&initializer.expr);
            let target = inferred_conversion_type(&typed.ty);
            if let Expr::MethodCall(call) = expression
                && call.method == "parse"
                && method_turbofish_type(call).is_none()
                && let Some(site) = self.resolve_expr(&call.receiver)
                && let Some(target) = target.clone()
            {
                self.record_conversion(site, target, &call.receiver);
            } else if let Expr::Call(call) = expression
                && let Expr::Path(function) = &*call.func
                && function.path.segments.last().is_some_and(|segment| {
                    matches!(
                        segment.ident.to_string().as_str(),
                        "from_str" | "parse" | "parse_str"
                    )
                })
                && let Some(first_argument) = call.args.first()
                && let Some(site) = self.resolve_expr(first_argument)
                && let Some(target) = target
            {
                self.record_conversion(site, target, first_argument);
            }
        }
        syn::visit::visit_local(self, local);
        self.record_local_binding(local);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        let expression = Expr::Path(path.clone());
        if let Some(site) = self.resolve_expr(&expression) {
            self.sites[site].use_sites.insert(use_site(&expression));
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        let expression = Expr::Field(field.clone());
        if let Some(site) = self.resolve_expr(&expression) {
            self.sites[site].use_sites.insert(use_site(&expression));
        }
        syn::visit::visit_expr_field(self, field);
    }

    fn visit_macro(&mut self, macro_node: &'ast syn::Macro) {
        if self.current_scope.is_some() {
            let diagnostic = macro_node
                .path
                .segments
                .last()
                .is_some_and(|segment| is_diagnostic_macro(&segment.ident.to_string()));
            let mut identifiers = Vec::new();
            collect_token_identifiers(macro_node.tokens.clone(), &mut identifiers);
            for (name, span) in identifiers {
                let start = span.start();
                if let Some(site) = self.resolve_scoped_name(&name, start.line) {
                    let use_site = (start.line, start.column);
                    self.sites[site].use_sites.insert(use_site);
                    if diagnostic {
                        self.sites[site].diagnostic_use_sites.insert(use_site);
                    }
                }
            }
        }
        syn::visit::visit_macro(self, macro_node);
    }
}

fn propagate_diagnostic_sinks(sites: &mut [Site]) {
    loop {
        let diagnostic_only: Vec<bool> = sites
            .iter()
            .map(|site| {
                !site.use_sites.is_empty()
                    && site
                        .use_sites
                        .iter()
                        .all(|location| site.diagnostic_use_sites.contains(location))
            })
            .collect();
        let mut changed = false;
        for site in sites.iter_mut() {
            for forwarded in &site.forwarded_uses {
                if diagnostic_only
                    .get(forwarded.target_site)
                    .copied()
                    .unwrap_or(false)
                {
                    changed |= site.diagnostic_use_sites.insert(forwarded.location);
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn annotate_shared_vocabularies(sites: &mut [Site], include_tests: bool) {
    let mut groups: HashMap<(String, Vec<String>), Vec<usize>> = HashMap::new();
    for (position, site) in sites.iter().enumerate() {
        if (!include_tests && site.is_test)
            || !(2..=16).contains(&site.literals.len())
            || site
                .literals
                .iter()
                .any(|literal| !is_enum_token_literal(literal))
            || is_open_text_role(&site.name)
        {
            continue;
        }
        let normalized_name = site.name.trim_start_matches("r#").to_ascii_lowercase();
        groups
            .entry((normalized_name, site.literals.iter().cloned().collect()))
            .or_default()
            .push(position);
    }

    for ((name, literals), positions) in groups {
        if positions.len() < 2 {
            continue;
        }
        // Repeated sentinel checks do not close an otherwise open domain. Two
        // `is_loopback_host` implementations comparing against localhost/IP
        // constants, for example, are not evidence that Host should be an
        // enum. Require at least one construction, call site, or match.
        if positions.iter().all(|position| {
            let site = &sites[*position];
            site.literal_observations == site.literal_comparisons
        }) {
            continue;
        }
        let declaration_count = positions.len();
        let type_name = shared_vocabulary_type_name(&sites[positions[0]], &name);
        let vocabulary = literals
            .iter()
            .take(6)
            .map(|literal| format!("{literal:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        for position in positions {
            let file_path = sites[position].file_path.clone();
            let line = sites[position].line;
            sites[position].shared_vocabulary = Some(SharedVocabulary {
                type_name: type_name.clone(),
                declaration_count,
            });
            sites[position].add_evidence(
                StringlyEvidenceKind::SharedVocabulary,
                format!(
                    "the same {name} vocabulary ({vocabulary}) appears in {declaration_count} declarations"
                ),
                &file_path,
                line,
            );
        }
    }
}

fn is_enum_token_literal(literal: &str) -> bool {
    !literal.is_empty()
        && literal.len() <= 64
        && !literal.chars().any(char::is_whitespace)
        && literal.chars().any(char::is_alphanumeric)
}

fn is_open_text_role(name: &str) -> bool {
    matches!(
        name.trim_start_matches("r#").to_ascii_lowercase().as_str(),
        "help"
            | "label"
            | "message"
            | "description"
            | "reason"
            | "text"
            | "what"
            | "name"
            | "field"
            | "preview"
            | "placeholder"
            | "example"
            | "sample"
    )
}

fn shared_vocabulary_type_name(site: &Site, normalized_name: &str) -> String {
    if normalized_name.chars().count() <= 2 || matches!(normalized_name, "key" | "value" | "id") {
        enum_name_for(site)
    } else {
        pascal_case(normalized_name)
    }
}

fn has_meaningful_vocabulary_role(site: &Site) -> bool {
    !matches!(
        site.name
            .trim_start_matches("r#")
            .to_ascii_lowercase()
            .as_str(),
        "s" | "s1" | "s2" | "arg" | "args" | "input" | "raw" | "str" | "string" | "text"
    )
}

#[derive(Debug)]
struct Candidate {
    suggestion: SuggestedType,
    confidence: f64,
    priority: u8,
    evidence: Option<StringlyEvidence>,
    source: CandidateSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateSource {
    Conversion,
    MatchTarget,
    StructuredLiteral,
    SharedVocabulary,
    LiteralVocabulary,
    SemanticName,
}

fn finalise_site(
    mut site: Site,
    catalog: &DependencyCatalog,
    imports: &HashMap<String, String>,
    local_types: &LocalTypeCatalog,
    min_confidence: f64,
) -> Option<StringlyFinding> {
    let mut candidates = Vec::new();
    let unmatched_supplied_literals: Vec<String> = site
        .supplied_literals
        .difference(&site.matched_literals)
        .cloned()
        .collect();
    let rejecting_match_is_closed =
        site.saw_rejecting_match && unmatched_supplied_literals.is_empty();
    if site.saw_rejecting_match && !unmatched_supplied_literals.is_empty() {
        let preview = unmatched_supplied_literals
            .iter()
            .take(5)
            .map(|literal| format!("{literal:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let omitted = unmatched_supplied_literals.len().saturating_sub(5);
        let file_path = site.file_path.clone();
        site.add_evidence(
            StringlyEvidenceKind::LiteralMatch,
            format!(
                "observed values outside the rejecting match ({preview}{}); treat it as a partial classifier, not a closed vocabulary",
                if omitted > 0 {
                    format!(", +{omitted} more")
                } else {
                    String::new()
                }
            ),
            &file_path,
            site.line,
        );
    }

    if site.conversion_targets.len() == 1 {
        let target = site.conversion_targets.iter().next().expect("one target");
        let conversion_dominates = site.use_sites.is_empty()
            || site
                .use_sites
                .iter()
                .all(|use_site| site.conversion_use_sites.contains(use_site));
        let confidence = if conversion_dominates { 0.99 } else { 0.82 };
        candidates.push(Candidate {
            suggestion: adapt_borrowed_suggestion(
                classify_type(target, imports, catalog, local_types),
                &site.declared_type,
            ),
            confidence,
            priority: 4,
            evidence: (!conversion_dominates).then(|| StringlyEvidence {
                kind: StringlyEvidenceKind::ExplicitConversion,
                message: format!(
                    "conversion to {target} does not dominate all observed uses; migration may need an adapter"
                ),
                file_path: site.file_path.clone(),
                line: site.line,
            }),
            source: CandidateSource::Conversion,
        });
    }

    if let Some(target) = common_match_target(&site.match_arm_targets) {
        candidates.push(Candidate {
            suggestion: classify_type(&target, imports, catalog, local_types),
            confidence: if rejecting_match_is_closed {
                0.98
            } else {
                0.90
            },
            priority: 5,
            evidence: Some(StringlyEvidence {
                kind: StringlyEvidenceKind::ExplicitConversion,
                message: format!(
                    "every literal match arm produces the existing nominal type {target}"
                ),
                file_path: site.file_path.clone(),
                line: site.line,
            }),
            source: CandidateSource::MatchTarget,
        });
    }

    if let Some(candidate) = structured_literal_candidate(&site, catalog, local_types) {
        candidates.push(candidate);
    }

    if site.literals.len() >= 2 {
        let (enum_name, confidence, priority, source) =
            if let Some(shared) = &site.shared_vocabulary {
                (
                    shared.type_name.clone(),
                    if shared.declaration_count >= 3 {
                        0.93
                    } else {
                        0.89
                    },
                    4,
                    CandidateSource::SharedVocabulary,
                )
            } else {
                let confidence = if rejecting_match_is_closed {
                    0.95
                } else if site.saw_literal_match {
                    0.80
                } else if site.literal_observations >= site.literals.len() * 2
                    && site.literals.len() <= 16
                    && site
                        .literals
                        .iter()
                        .all(|literal| is_enum_token_literal(literal))
                    && !is_open_text_role(&site.name)
                    && has_meaningful_vocabulary_role(&site)
                {
                    0.86
                } else {
                    0.78
                };
                (
                    enum_name_for(&site),
                    confidence,
                    3,
                    CandidateSource::LiteralVocabulary,
                )
            };
        candidates.push(Candidate {
            suggestion: local_suggestion(
                enum_name.clone(),
                local_types.can_replace_string(last_path_segment(&enum_name)),
            ),
            confidence,
            priority,
            evidence: None,
            source,
        });
    }

    if let Some(candidate) = semantic_name_candidate(&site, catalog, local_types) {
        candidates.push(candidate);
    }

    let candidate = candidates.into_iter().max_by(|left, right| {
        left.confidence
            .total_cmp(&right.confidence)
            .then_with(|| left.priority.cmp(&right.priority))
    })?;
    if candidate.confidence < min_confidence {
        return None;
    }
    let mut caveats = caveats_for_site(&site, &candidate);
    caveats.sort_by(|left, right| {
        caveat_rank(left.kind)
            .cmp(&caveat_rank(right.kind))
            .then_with(|| left.message.cmp(&right.message))
    });
    if let Some(evidence) = candidate.evidence {
        site.evidence.push(evidence);
    }
    site.evidence.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| evidence_rank(left.kind).cmp(&evidence_rank(right.kind)))
            .then_with(|| left.message.cmp(&right.message))
    });

    let has_literal_details = matches!(
        candidate.source,
        CandidateSource::MatchTarget
            | CandidateSource::StructuredLiteral
            | CandidateSource::SharedVocabulary
            | CandidateSource::LiteralVocabulary
    );
    let literal_stats = has_literal_details.then_some(StringlyLiteralStats {
        distinct: site.literals.len(),
        observations: site.literal_observations,
        comparisons: site.literal_comparisons,
        matches: site.literal_matches,
        constructions: site.literal_constructions,
        call_sites: site.literal_call_sites,
    });

    Some(StringlyFinding {
        file_path: site.file_path,
        line: site.line,
        name: site.name,
        owner: site.owner,
        site_kind: site.site_kind,
        declared_type: site.declared_type,
        suggestion: candidate.suggestion,
        confidence: candidate.confidence,
        evidence: site.evidence,
        literals: if has_literal_details {
            site.literals.into_iter().collect()
        } else {
            Vec::new()
        },
        literal_stats,
        caveats,
        is_test: site.is_test,
    })
}

fn caveats_for_site(site: &Site, candidate: &Candidate) -> Vec<StringlyCaveat> {
    let mut caveats = Vec::new();
    match site.provenance {
        FileProvenance::Project => {}
        FileProvenance::Generated => caveats.push(caveat(
            StringlyCaveatKind::GeneratedCode,
            "declaration is in generated code; change its schema or generator instead".to_string(),
        )),
        FileProvenance::Vendored => caveats.push(caveat(
            StringlyCaveatKind::VendoredCode,
            "declaration is in vendored third-party code".to_string(),
        )),
    }

    if site.trait_contract {
        caveats.push(caveat(
            StringlyCaveatKind::TraitContract,
            "parameter type is fixed by a trait contract; changing only this signature would not compile"
                .to_string(),
        ));
    }
    if let Some(reason) = text_boundary_reason(site, candidate) {
        caveats.push(caveat(StringlyCaveatKind::TextBoundary, reason));
    }
    if candidate.source == CandidateSource::Conversion
        && is_implementation_machinery(&candidate.suggestion.canonical_path)
    {
        caveats.push(caveat(
            StringlyCaveatKind::ImplementationDetail,
            format!(
                "{} is parser/adapter/representation machinery, not a stable domain value type",
                candidate.suggestion.display
            ),
        ));
    }
    if site.public_api {
        caveats.push(caveat(
            StringlyCaveatKind::PublicApi,
            "changing this public declaration can require a semver-breaking caller migration"
                .to_string(),
        ));
    }
    if site.external_representation {
        caveats.push(caveat(
            StringlyCaveatKind::ExternalRepresentation,
            "field participates in serialization or persistence; preserve its wire/database representation with an adapter"
                .to_string(),
        ));
    }
    caveats
}

fn caveat(kind: StringlyCaveatKind, message: String) -> StringlyCaveat {
    StringlyCaveat {
        kind,
        message,
        suppresses_by_default: kind.suppresses_by_default(),
    }
}

fn text_boundary_reason(site: &Site, candidate: &Candidate) -> Option<String> {
    if candidate.source == CandidateSource::SemanticName {
        return None;
    }
    if candidate.source == CandidateSource::LiteralVocabulary
        && !site.saw_literal_match
        && let Some(reason) = literal_text_boundary_reason(site)
    {
        return Some(reason);
    }
    if site.site_kind != StringSiteKind::FunctionParameter {
        return None;
    }
    let function_name = site.function_name.as_deref()?;
    if let Some(role) = boundary_function_role(function_name) {
        return Some(format!(
            "`{function_name}` is a {role}; its textual input is intentional and the stronger type belongs after this boundary"
        ));
    }

    let normalized_function = function_name.trim_start_matches("r#").to_ascii_lowercase();
    if candidate.source == CandidateSource::Conversion
        && normalized_function
            .split('_')
            .next()
            .is_some_and(|first| first == "validate")
    {
        return Some(format!(
            "`{function_name}` validates textual input; parsing `{}` is part of that validation boundary",
            candidate.suggestion.display
        ));
    }
    let parameter_name = site.name.trim_start_matches("r#").to_ascii_lowercase();
    let target = last_path_segment(&candidate.suggestion.canonical_path);
    if candidate.source == CandidateSource::Conversion
        && ((candidate
            .suggestion
            .canonical_path
            .starts_with("serde_json::")
            && parameter_name.contains("json"))
            || (target == "Claims"
                && (parameter_name.contains("jwt") || parameter_name.contains("token"))))
    {
        return Some(format!(
            "`{}` is an encoded payload decoded into `{target}` at this boundary",
            site.name
        ));
    }

    if site.return_type_names.contains(target) {
        return Some(format!(
            "this function turns textual input into `{target}` in its return type; moving the same conversion to every caller would not remove the boundary"
        ));
    }

    None
}

fn literal_text_boundary_reason(site: &Site) -> Option<String> {
    let prose_literals = site
        .literals
        .iter()
        .filter(|literal| literal.chars().any(char::is_whitespace))
        .count();
    if prose_literals > 0 && prose_literals * 2 >= site.literals.len() {
        return Some(format!(
            "{prose_literals} of {} observed literals contain whitespace and look like prose/diagnostic labels, not a closed value vocabulary",
            site.literals.len()
        ));
    }

    let one_label_per_call = site.literal_call_sites > 0
        && site.literal_observations == site.literals.len()
        && site.literal_call_sites == site.literal_observations;
    if one_label_per_call && is_open_text_role(&site.name) {
        return Some(format!(
            "`{}` receives one distinct literal per call site, which is characteristic of an open label space",
            site.name
        ));
    }

    if !site.use_sites.is_empty()
        && site
            .use_sites
            .iter()
            .all(|use_site| site.diagnostic_use_sites.contains(use_site))
    {
        return Some(format!(
            "`{}` flows only into formatting, tracing, or error construction; it is diagnostic text rather than a domain value",
            site.name
        ));
    }
    None
}

fn boundary_function_role(name: &str) -> Option<&'static str> {
    let normalized = name.trim_start_matches("r#").to_ascii_lowercase();
    let first = normalized.split('_').next().unwrap_or(&normalized);
    if matches!(
        first,
        "parse"
            | "decode"
            | "deserialize"
            | "deserialise"
            | "unmarshal"
            | "coerce"
            | "convert"
            | "transform"
            | "lex"
            | "tokenize"
            | "tokenise"
            | "import"
            | "ingest"
    ) || normalized.starts_with("try_parse_")
    {
        return Some("parser/decoder/ingestion boundary");
    }
    if first == "from" {
        return Some("text-to-value constructor boundary");
    }
    if first == "map" || normalized.contains("_to_") || normalized.contains("_from_") {
        return Some("mapping/adapter boundary");
    }
    None
}

fn is_implementation_machinery(target: &str) -> bool {
    if matches!(
        target,
        "serde_json::Value"
            | "serde_json::Map"
            | "toml::Value"
            | "wasm_bindgen::JsValue"
            | "js_sys::JsString"
    ) {
        return true;
    }
    matches!(
        last_path_segment(target),
        "Parser"
            | "Deserializer"
            | "Serializer"
            | "Decoder"
            | "Encoder"
            | "Lexer"
            | "Tokenizer"
            | "Tokeniser"
            | "Builder"
            | "Reader"
            | "Writer"
    )
}

fn structured_literal_candidate(
    site: &Site,
    catalog: &DependencyCatalog,
    local_types: &LocalTypeCatalog,
) -> Option<Candidate> {
    let normalized_name = site.name.trim_start_matches("r#").to_ascii_lowercase();
    let status_role = normalized_name == "status"
        || normalized_name.ends_with("_status")
        || normalized_name == "status_line";
    if site.literals.len() >= 2
        && status_role
        && site
            .literals
            .iter()
            .all(|literal| is_http_status_line(literal))
    {
        return Some(Candidate {
            suggestion: dependency_suggestion("http", "StatusCode", catalog),
            confidence: 0.96,
            priority: 6,
            evidence: Some(StringlyEvidence {
                kind: StringlyEvidenceKind::StructuredLiteral,
                message: "all observed literals are HTTP status lines; use the protocol type and render the reason phrase at the wire boundary"
                    .to_string(),
                file_path: site.file_path.clone(),
                line: site.line,
            }),
            source: CandidateSource::StructuredLiteral,
        });
    }

    let endpoint_role = matches!(
        normalized_name.as_str(),
        "path" | "endpoint" | "route" | "resource_path"
    );
    let filesystem_path_role = normalized_name == "path" || normalized_name.ends_with("_path");
    if filesystem_path_role
        && site.literals.len() >= 2
        && site
            .literals
            .iter()
            .all(|literal| is_absolute_filesystem_path_literal(literal))
    {
        let suggestion = if site.declared_type.trim_start().starts_with('&') {
            let mut suggestion = standard_suggestion("std::path::Path");
            suggestion.display = borrowed_path_display(&site.declared_type);
            suggestion
        } else {
            standard_suggestion("std::path::PathBuf")
        };
        return Some(Candidate {
            suggestion,
            confidence: 0.94,
            priority: 6,
            evidence: Some(StringlyEvidence {
                kind: StringlyEvidenceKind::StructuredLiteral,
                message:
                    "all observed values are absolute filesystem paths under conventional OS roots"
                        .to_string(),
                file_path: site.file_path.clone(),
                line: site.line,
            }),
            source: CandidateSource::StructuredLiteral,
        });
    }
    if endpoint_role
        && site.literals.len() >= 2
        && site
            .literals
            .iter()
            .all(|literal| is_absolute_endpoint_literal(literal))
    {
        let type_name = if normalized_name == "endpoint" {
            pascal_case(&site.name)
        } else {
            "Endpoint".to_string()
        };
        return Some(Candidate {
            suggestion: local_suggestion(
                type_name.clone(),
                local_types.can_replace_string(&type_name),
            ),
            confidence: 0.90,
            priority: 5,
            evidence: Some(StringlyEvidence {
                kind: StringlyEvidenceKind::StructuredLiteral,
                message: format!(
                    "all {} observed values are absolute endpoint paths in a bounded call-site vocabulary",
                    site.literals.len()
                ),
                file_path: site.file_path.clone(),
                line: site.line,
            }),
            source: CandidateSource::StructuredLiteral,
        });
    }
    None
}

fn is_http_status_line(literal: &str) -> bool {
    let bytes = literal.as_bytes();
    if bytes.len() < 3 || !bytes[..3].iter().all(u8::is_ascii_digit) {
        return false;
    }
    let Ok(code) = literal[..3].parse::<u16>() else {
        return false;
    };
    (100..=599).contains(&code)
        && (bytes.len() == 3 || (bytes[3].is_ascii_whitespace() && !literal[3..].trim().is_empty()))
}

fn is_absolute_endpoint_literal(literal: &str) -> bool {
    literal.starts_with('/')
        && !literal.starts_with("//")
        && literal.len() > 1
        && literal.len() <= 256
        && !literal.chars().any(char::is_whitespace)
        && !is_absolute_filesystem_path_literal(literal)
}

fn is_absolute_filesystem_path_literal(literal: &str) -> bool {
    if !literal.starts_with('/') || literal.starts_with("//") {
        return false;
    }
    let first_segment = literal
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        first_segment.as_str(),
        "bin"
            | "boot"
            | "dev"
            | "etc"
            | "home"
            | "lib"
            | "lib64"
            | "media"
            | "mnt"
            | "opt"
            | "proc"
            | "root"
            | "run"
            | "sbin"
            | "srv"
            | "sys"
            | "tmp"
            | "usr"
            | "var"
    )
}

fn semantic_name_candidate(
    site: &Site,
    catalog: &DependencyCatalog,
    local_types: &LocalTypeCatalog,
) -> Option<Candidate> {
    let name = site.name.trim_start_matches("r#").to_ascii_lowercase();
    let semantic = |message: String| {
        Some(StringlyEvidence {
            kind: StringlyEvidenceKind::SemanticName,
            message,
            file_path: site.file_path.clone(),
            line: site.line,
        })
    };

    let (suggestion, confidence, message) = if name == "path" || name.ends_with("_path") {
        let suggestion = if site.declared_type.trim_start().starts_with('&') {
            let mut suggestion = standard_suggestion("std::path::Path");
            suggestion.display = borrowed_path_display(&site.declared_type);
            suggestion
        } else {
            standard_suggestion("std::path::PathBuf")
        };
        (
            suggestion,
            0.74,
            format!("name `{}` has a path-specific suffix", site.name),
        )
    } else if matches!(name.as_str(), "ip" | "ip_addr" | "ip_address")
        || name.ends_with("_ip_addr")
        || name.ends_with("_ip_address")
    {
        (
            standard_suggestion("std::net::IpAddr"),
            0.80,
            format!("name `{}` identifies an IP address", site.name),
        )
    } else if matches!(name.as_str(), "socket_addr" | "bind_addr" | "listen_addr")
        || name.ends_with("_socket_addr")
    {
        (
            standard_suggestion("std::net::SocketAddr"),
            0.80,
            format!("name `{}` identifies a socket address", site.name),
        )
    } else if name == "url" || name.ends_with("_url") {
        (
            dependency_suggestion("url", "Url", catalog),
            0.82,
            format!("name `{}` identifies a URL", site.name),
        )
    } else if name == "uuid" || name.ends_with("_uuid") {
        (
            dependency_suggestion("uuid", "Uuid", catalog),
            0.82,
            format!("name `{}` identifies a UUID", site.name),
        )
    } else if name == "uri" || name.ends_with("_uri") {
        (
            dependency_suggestion("http", "Uri", catalog),
            0.78,
            format!("name `{}` identifies a URI", site.name),
        )
    } else if name == "version" || name.ends_with("_version") {
        (
            dependency_suggestion("semver", "Version", catalog),
            0.74,
            format!("name `{}` looks like a semantic version", site.name),
        )
    } else if name.ends_with("_id") {
        let type_name = pascal_case(&name);
        (
            local_suggestion(
                type_name.clone(),
                local_types.can_replace_string(last_path_segment(&type_name)),
            ),
            0.72,
            format!("name `{}` is an opaque identifier candidate", site.name),
        )
    } else {
        return None;
    };

    Some(Candidate {
        suggestion,
        confidence,
        priority: 2,
        evidence: semantic(message),
        source: CandidateSource::SemanticName,
    })
}

fn adapt_borrowed_suggestion(mut suggestion: SuggestedType, declared_type: &str) -> SuggestedType {
    if !declared_type.trim_start().starts_with('&') {
        return suggestion;
    }
    let borrowed_canonical = match suggestion.canonical_path.as_str() {
        "std::path::PathBuf" => Some("std::path::Path"),
        "std::ffi::OsString" => Some("std::ffi::OsStr"),
        "std::ffi::CString" => Some("std::ffi::CStr"),
        _ => None,
    };
    if let Some(canonical) = borrowed_canonical {
        suggestion.canonical_path = canonical.to_string();
        suggestion.display = declared_type.trim().strip_suffix("str").map_or_else(
            || format!("&{canonical}"),
            |prefix| format!("{prefix}{canonical}"),
        );
    }
    suggestion
}

fn classify_type(
    raw_target: &str,
    imports: &HashMap<String, String>,
    catalog: &DependencyCatalog,
    local_types: &LocalTypeCatalog,
) -> SuggestedType {
    let mut target = normalize_type_path(raw_target);
    if !target.contains("::")
        && let Some(imported) = imports.get(&target)
    {
        target = imported.clone();
    }

    let short = last_path_segment(&target);
    if target.starts_with("crate::")
        || target.starts_with("self::")
        || target.starts_with("super::")
    {
        let exists = local_types.contains(short);
        return local_suggestion(target, exists);
    }

    // An unqualified project type may intentionally shadow a familiar std or
    // ecosystem type. Imported types were expanded above, so local wins only
    // for the genuinely unqualified spelling.
    if !target.contains("::") && local_types.contains(short) {
        return local_suggestion(target, true);
    }

    if let Some(canonical) = canonical_standard_type(&target) {
        return standard_suggestion(&canonical);
    }

    if !target.contains("::")
        && let Some((package, type_name)) = known_external_type(&target)
    {
        return dependency_suggestion(package, type_name, catalog);
    }

    let root = target.split("::").next().unwrap_or(&target).to_string();
    if let Some(package) = catalog.package_for_alias(&root) {
        let canonical_root = normalize_crate_name(package);
        let canonical = replace_path_root(&target, &canonical_root);
        return SuggestedType {
            display: target,
            canonical_path: canonical,
            origin: TypeOrigin::ExistingDependency,
            crate_name: Some(package.to_string()),
            dependency_alias: (root != canonical_root).then_some(root),
            new_dependency: false,
            local_type_exists: false,
        };
    }

    // Qualified local module paths (for example `domain::UserId`) are not
    // distinguishable from crate roots syntactically. A matching project type
    // is stronger evidence than treating the root as an undeclared crate.
    if local_types.contains(short) {
        return local_suggestion(target, true);
    }

    if target.contains("::") && root.chars().next().is_some_and(char::is_lowercase) {
        let package = normalize_crate_name(&root);
        return SuggestedType {
            display: target.clone(),
            canonical_path: target,
            origin: TypeOrigin::ExternalCrate,
            crate_name: Some(package),
            dependency_alias: None,
            new_dependency: true,
            local_type_exists: false,
        };
    }

    local_suggestion(target, false)
}

fn standard_suggestion(canonical_path: &str) -> SuggestedType {
    SuggestedType {
        display: canonical_path.to_string(),
        canonical_path: canonical_path.to_string(),
        origin: TypeOrigin::StandardLibrary,
        crate_name: None,
        dependency_alias: None,
        new_dependency: false,
        local_type_exists: false,
    }
}

fn local_suggestion(type_name: String, local_type_exists: bool) -> SuggestedType {
    SuggestedType {
        display: type_name.clone(),
        canonical_path: type_name,
        origin: TypeOrigin::LocalType,
        crate_name: None,
        dependency_alias: None,
        new_dependency: false,
        local_type_exists,
    }
}

fn dependency_suggestion(
    package: &str,
    type_name: &str,
    catalog: &DependencyCatalog,
) -> SuggestedType {
    let package_name = package.to_string();
    let canonical_root = normalize_crate_name(package);
    let canonical_path = format!("{}::{}", canonical_root, type_name);
    if let Some(alias) = catalog.preferred_alias(&package_name) {
        SuggestedType {
            display: format!("{}::{}", alias, type_name),
            canonical_path,
            origin: TypeOrigin::ExistingDependency,
            crate_name: Some(package_name),
            dependency_alias: (alias != canonical_root).then(|| alias.to_string()),
            new_dependency: false,
            local_type_exists: false,
        }
    } else {
        SuggestedType {
            display: canonical_path.clone(),
            canonical_path,
            origin: TypeOrigin::ExternalCrate,
            crate_name: Some(package_name),
            dependency_alias: None,
            new_dependency: true,
            local_type_exists: false,
        }
    }
}

fn canonical_standard_type(target: &str) -> Option<String> {
    let short = last_path_segment(target);
    if target.starts_with("std::") || target.starts_with("core::") || target.starts_with("alloc::")
    {
        let canonical = match short {
            "Path" => "std::path::Path",
            "PathBuf" => "std::path::PathBuf",
            "IpAddr" => "std::net::IpAddr",
            "Ipv4Addr" => "std::net::Ipv4Addr",
            "Ipv6Addr" => "std::net::Ipv6Addr",
            "SocketAddr" => "std::net::SocketAddr",
            "OsStr" => "std::ffi::OsStr",
            "OsString" => "std::ffi::OsString",
            "CStr" => "std::ffi::CStr",
            "CString" => "std::ffi::CString",
            "Duration" => "std::time::Duration",
            "SystemTime" => "std::time::SystemTime",
            _ => target,
        };
        return Some(canonical.to_string());
    }
    let canonical = match short {
        "Path" => Some("std::path::Path"),
        "PathBuf" => Some("std::path::PathBuf"),
        "IpAddr" => Some("std::net::IpAddr"),
        "Ipv4Addr" => Some("std::net::Ipv4Addr"),
        "Ipv6Addr" => Some("std::net::Ipv6Addr"),
        "SocketAddr" => Some("std::net::SocketAddr"),
        "OsStr" => Some("std::ffi::OsStr"),
        "OsString" => Some("std::ffi::OsString"),
        "CStr" => Some("std::ffi::CStr"),
        "CString" => Some("std::ffi::CString"),
        "Duration" => Some("std::time::Duration"),
        "SystemTime" => Some("std::time::SystemTime"),
        "bool" => Some("bool"),
        "char" => Some("char"),
        "u8" => Some("u8"),
        "u16" => Some("u16"),
        "u32" => Some("u32"),
        "u64" => Some("u64"),
        "u128" => Some("u128"),
        "usize" => Some("usize"),
        "i8" => Some("i8"),
        "i16" => Some("i16"),
        "i32" => Some("i32"),
        "i64" => Some("i64"),
        "i128" => Some("i128"),
        "isize" => Some("isize"),
        "f32" => Some("f32"),
        "f64" => Some("f64"),
        _ => None,
    };
    canonical.map(str::to_string)
}

fn known_external_type(short: &str) -> Option<(&'static str, &'static str)> {
    match short {
        "Uuid" => Some(("uuid", "Uuid")),
        "Url" => Some(("url", "Url")),
        "Version" => Some(("semver", "Version")),
        "Uri" => Some(("http", "Uri")),
        "Method" => Some(("http", "Method")),
        "StatusCode" => Some(("http", "StatusCode")),
        "HeaderName" => Some(("http", "HeaderName")),
        "HeaderValue" => Some(("http", "HeaderValue")),
        _ => None,
    }
}

fn collect_imports(file: &syn::File) -> HashMap<String, String> {
    #[derive(Default)]
    struct ImportCollector {
        imports: HashMap<String, String>,
    }
    impl<'ast> Visit<'ast> for ImportCollector {
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            flatten_use_tree(Vec::new(), &item.tree, &mut self.imports);
        }
    }

    let mut collector = ImportCollector::default();
    collector.visit_file(file);
    collector.imports
}

fn flatten_use_tree(
    mut prefix: Vec<String>,
    tree: &UseTree,
    imports: &mut HashMap<String, String>,
) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            flatten_use_tree(prefix, &path.tree, imports);
        }
        UseTree::Name(name) => {
            prefix.push(name.ident.to_string());
            imports.insert(name.ident.to_string(), prefix.join("::"));
        }
        UseTree::Rename(rename) => {
            prefix.push(rename.ident.to_string());
            imports.insert(rename.rename.to_string(), prefix.join("::"));
        }
        UseTree::Group(group) => {
            for nested in &group.items {
                flatten_use_tree(prefix.clone(), nested, imports);
            }
        }
        UseTree::Glob(_) => {}
    }
}

fn conversion_target_from_path(path: &syn::Path) -> Option<String> {
    let last = path.segments.last()?;
    let leaf = last.ident.to_string();
    if !matches!(
        leaf.as_str(),
        "from" | "try_from" | "from_str" | "parse" | "parse_str" | "decode" | "deserialize"
    ) {
        return None;
    }
    // A turbofish identifies an output type only for known conversion APIs.
    // Previously this ran before the leaf-name check, so an arbitrary
    // `foo::<T>(text)` was incorrectly treated as a conversion into `T`.
    if let Some(target) = generic_type_argument(&last.arguments) {
        return Some(target);
    }
    let qualifier: Vec<String> = path
        .segments
        .iter()
        .take(path.segments.len().saturating_sub(1))
        .map(|segment| segment.ident.to_string())
        .collect();
    let type_name = qualifier.last()?;
    if matches!(type_name.as_str(), "From" | "TryFrom" | "FromStr") {
        return None;
    }
    if !type_name.chars().next().is_some_and(char::is_uppercase) {
        return None;
    }
    Some(qualifier.join("::"))
}

fn method_turbofish_type(call: &ExprMethodCall) -> Option<String> {
    call.turbofish
        .as_ref()?
        .args
        .iter()
        .find_map(|argument| match argument {
            GenericArgument::Type(ty) => Some(render_tokens(ty)),
            _ => None,
        })
}

fn generic_type_argument(arguments: &PathArguments) -> Option<String> {
    let PathArguments::AngleBracketed(arguments) = arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(render_tokens(ty)),
        _ => None,
    })
}

fn inferred_conversion_type(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return (!matches!(ty, Type::Infer(_))).then(|| render_tokens(ty));
    };
    let Some(last) = path.path.segments.last() else {
        return Some(render_tokens(ty));
    };
    if !matches!(last.ident.to_string().as_str(), "Result" | "Option") {
        return Some(render_tokens(ty));
    }
    let PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(Type::Infer(_)) => None,
        GenericArgument::Type(inner) => Some(render_tokens(inner)),
        _ => None,
    })
}

fn string_literals_in_pattern(pattern: &Pat) -> Vec<String> {
    match pattern {
        Pat::Lit(literal) => match &literal.lit {
            Lit::Str(value) => vec![value.value()],
            _ => Vec::new(),
        },
        Pat::Or(or_pattern) => or_pattern
            .cases
            .iter()
            .flat_map(string_literals_in_pattern)
            .collect(),
        Pat::Paren(paren) => string_literals_in_pattern(&paren.pat),
        Pat::Reference(reference) => string_literals_in_pattern(&reference.pat),
        _ => Vec::new(),
    }
}

fn returned_nominal_types(
    expression: &Expr,
    self_type: Option<&str>,
    enclosing_return_types: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    match peel_control_flow(expression) {
        Expr::Path(path) => {
            if let Some(owner) = nominal_path_owner(&path.path, self_type, enclosing_return_types) {
                targets.insert(owner);
            }
        }
        Expr::Struct(struct_expr) => {
            if let Some(target) = struct_expr
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .filter(|name| starts_uppercase(name))
            {
                targets.insert(target);
            }
        }
        Expr::Call(call) => {
            if let Expr::Path(function) = &*call.func {
                let leaf = function
                    .path
                    .segments
                    .last()
                    .map(|segment| segment.ident.to_string());
                if leaf
                    .as_deref()
                    .is_some_and(|leaf| matches!(leaf, "Ok" | "Some"))
                {
                    for argument in &call.args {
                        targets.extend(returned_nominal_types(
                            argument,
                            self_type,
                            enclosing_return_types,
                        ));
                    }
                } else if function.path.segments.len() == 1
                    && starts_uppercase(&function.path.segments[0].ident.to_string())
                {
                    targets.insert(function.path.segments[0].ident.to_string());
                } else if let Some(owner) =
                    nominal_path_owner(&function.path, self_type, enclosing_return_types)
                {
                    targets.insert(owner);
                }
            }
        }
        Expr::Return(returned) => {
            if let Some(inner) = &returned.expr {
                targets.extend(returned_nominal_types(
                    inner,
                    self_type,
                    enclosing_return_types,
                ));
            }
        }
        Expr::Block(block) => {
            if let Some(syn::Stmt::Expr(tail, _)) = block.block.stmts.last() {
                targets.extend(returned_nominal_types(
                    tail,
                    self_type,
                    enclosing_return_types,
                ));
            }
        }
        Expr::Tuple(tuple) => {
            for element in &tuple.elems {
                targets.extend(returned_nominal_types(
                    element,
                    self_type,
                    enclosing_return_types,
                ));
            }
        }
        _ => {}
    }
    targets
}

fn nominal_path_owner(
    path: &syn::Path,
    self_type: Option<&str>,
    enclosing_return_types: &BTreeSet<String>,
) -> Option<String> {
    if path.segments.len() < 2 {
        return None;
    }
    let member = path.segments.last()?.ident.to_string();
    let owner = path.segments.iter().rev().nth(1)?.ident.to_string();
    let owner = if owner == "Self" {
        self_type?.to_string()
    } else if starts_uppercase(&owner) {
        owner
    } else {
        return None;
    };
    // `Type::Variant` and `Type::Variant(...)` are nominal constructions.
    // A lowercase associated call (`Type::parse()`, `Math::sin()`) is not:
    // accept it only when the enclosing signature independently names Type as
    // a return type. This keeps constructor-style APIs without guessing that
    // every static method returns its receiver type.
    (starts_uppercase(&member) || enclosing_return_types.contains(&owner)).then_some(owner)
}

fn starts_uppercase(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn common_match_target(arm_targets: &[BTreeSet<String>]) -> Option<String> {
    if arm_targets.len() < 2 || arm_targets.iter().any(BTreeSet::is_empty) {
        return None;
    }
    let mut common = arm_targets.first()?.clone();
    for targets in &arm_targets[1..] {
        common.retain(|target| targets.contains(target));
    }
    (common.len() == 1).then(|| common.into_iter().next().expect("one common target"))
}

fn extract_string_literal(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Lit(literal) => match &literal.lit {
            Lit::Str(value) => Some(value.value()),
            _ => None,
        },
        Expr::Reference(reference) => extract_string_literal(&reference.expr),
        Expr::Paren(paren) => extract_string_literal(&paren.expr),
        Expr::Group(group) => extract_string_literal(&group.expr),
        Expr::MethodCall(call)
            if call.args.is_empty()
                && matches!(
                    call.method.to_string().as_str(),
                    "to_string" | "to_owned" | "into"
                ) =>
        {
            extract_string_literal(&call.receiver)
        }
        Expr::Call(call) if call.args.len() == 1 => {
            let Expr::Path(function) = &*call.func else {
                return None;
            };
            let leaf = function.path.segments.last()?.ident.to_string();
            if matches!(leaf.as_str(), "from" | "from_str") {
                call.args.first().and_then(extract_string_literal)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn literal_preview(literal: &str) -> String {
    const MAX_LITERAL_CHARS: usize = 120;
    let mut chars = literal.chars();
    let mut preview: String = chars.by_ref().take(MAX_LITERAL_CHARS).collect();
    if chars.next().is_some() {
        preview.push('…');
    }
    preview
}

fn peel_control_flow(expression: &Expr) -> &Expr {
    match expression {
        Expr::Try(try_expr) => peel_control_flow(&try_expr.expr),
        Expr::Await(await_expr) => peel_control_flow(&await_expr.base),
        Expr::Paren(paren) => peel_control_flow(&paren.expr),
        Expr::Group(group) => peel_control_flow(&group.expr),
        Expr::MethodCall(call)
            if matches!(call.method.to_string().as_str(), "unwrap" | "expect") =>
        {
            peel_control_flow(&call.receiver)
        }
        _ => expression,
    }
}

fn use_site(expression: &Expr) -> (usize, usize) {
    let expression = match expression {
        Expr::Reference(reference) => return use_site(&reference.expr),
        Expr::Paren(paren) => return use_site(&paren.expr),
        Expr::Group(group) => return use_site(&group.expr),
        Expr::Try(try_expr) => return use_site(&try_expr.expr),
        Expr::Await(await_expr) => return use_site(&await_expr.base),
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            return use_site(&unary.expr);
        }
        Expr::MethodCall(call)
            if call.args.is_empty()
                && matches!(
                    call.method.to_string().as_str(),
                    "as_str" | "as_ref" | "as_deref" | "borrow" | "trim"
                ) =>
        {
            return use_site(&call.receiver);
        }
        _ => expression,
    };
    let start = expression.span().start();
    (start.line, start.column)
}

fn collect_token_identifiers(stream: TokenStream, identifiers: &mut Vec<(String, Span)>) {
    for token in stream {
        match token {
            TokenTree::Ident(ident) => identifiers.push((ident.to_string(), ident.span())),
            TokenTree::Group(group) => collect_token_identifiers(group.stream(), identifiers),
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

fn is_diagnostic_macro(name: &str) -> bool {
    matches!(
        name,
        "format"
            | "format_args"
            | "write"
            | "writeln"
            | "print"
            | "println"
            | "eprint"
            | "eprintln"
            | "debug"
            | "info"
            | "warn"
            | "error"
            | "trace"
            | "panic"
            | "bail"
            | "ensure"
    )
}

fn is_diagnostic_constructor_path(path: &syn::Path) -> bool {
    let Some(leaf) = path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
    else {
        return false;
    };
    if leaf == "Err" {
        return true;
    }
    path.segments
        .iter()
        .rev()
        .nth(1)
        .map(|segment| segment.ident.to_string())
        .is_some_and(|owner| owner == "Error" || owner.ends_with("Error"))
}

fn is_catch_all_pattern(pattern: &Pat) -> bool {
    match pattern {
        Pat::Wild(_) | Pat::Ident(_) => true,
        Pat::Paren(paren) => is_catch_all_pattern(&paren.pat),
        Pat::Reference(reference) => is_catch_all_pattern(&reference.pat),
        Pat::Or(or_pattern) => or_pattern.cases.iter().any(is_catch_all_pattern),
        _ => false,
    }
}

fn is_rejecting_expr(expression: &Expr) -> bool {
    match peel_control_flow(expression) {
        Expr::Return(return_expr) => return_expr.expr.as_deref().is_none_or(is_rejecting_expr),
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "None"),
        Expr::Call(call) => match &*call.func {
            Expr::Path(path) => path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Err"),
            _ => false,
        },
        Expr::Lit(literal) => matches!(&literal.lit, Lit::Bool(value) if !value.value),
        Expr::Macro(macro_expr) => macro_expr.mac.path.segments.last().is_some_and(|segment| {
            matches!(
                segment.ident.to_string().as_str(),
                "panic" | "unreachable" | "todo" | "unimplemented"
            )
        }),
        Expr::Block(block) => block
            .block
            .stmts
            .last()
            .is_some_and(|statement| match statement {
                syn::Stmt::Expr(expression, _) => is_rejecting_expr(expression),
                _ => false,
            }),
        _ => false,
    }
}

fn is_string_type(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => {
            let segments = &path.path.segments;
            (segments.len() == 1 && segments[0].ident == "String")
                || path_is_one_of(&path.path, &["std", "string", "String"])
                || path_is_one_of(&path.path, &["alloc", "string", "String"])
        }
        Type::Reference(reference) => match &*reference.elem {
            Type::Path(path) => {
                let segments = &path.path.segments;
                (segments.len() == 1 && segments[0].ident == "str")
                    || path_is_one_of(&path.path, &["std", "primitive", "str"])
                    || path_is_one_of(&path.path, &["core", "primitive", "str"])
            }
            _ => false,
        },
        _ => false,
    }
}

fn path_is_one_of(path: &syn::Path, expected: &[&str]) -> bool {
    path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment.ident == *expected)
}

fn is_textual_target(target: &str) -> bool {
    matches!(last_path_segment(target), "String" | "str" | "Cow")
}

fn simple_pat_name(pattern: &Pat) -> Option<String> {
    match pattern {
        Pat::Ident(ident) => Some(ident.ident.to_string()),
        Pat::Reference(reference) => simple_pat_name(&reference.pat),
        _ => None,
    }
}

fn type_head(ty: &Type) -> String {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "<impl>".to_string()),
        Type::Reference(reference) => type_head(&reference.elem),
        _ => render_tokens(ty),
    }
}

fn struct_expression_owner(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Struct(structure) => structure
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Expr::Reference(reference) => struct_expression_owner(&reference.expr),
        Expr::Paren(paren) => struct_expression_owner(&paren.expr),
        Expr::Group(group) => struct_expression_owner(&group.expr),
        _ => None,
    }
}

fn scope_id(file_path: &str, span: Span) -> String {
    format!("{}:{}", file_path, span.start().line)
}

fn unique_in_file(candidates: &[usize], sites: &[Site], file_path: &str) -> Option<usize> {
    if candidates.len() == 1 {
        return candidates.first().copied();
    }
    let mut same_file = candidates
        .iter()
        .copied()
        .filter(|position| sites[*position].file_path == file_path);
    let first = same_file.next()?;
    same_file.next().is_none().then_some(first)
}

fn is_self_expr(expression: &Expr) -> bool {
    matches!(
        expression,
        Expr::Path(path)
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && path.path.segments[0].ident == "self"
    )
}

fn has_test_attr(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        is_test_attribute_path(attribute.path())
            || (attribute.path().is_ident("cfg")
                && attribute
                    .parse_args::<Meta>()
                    .is_ok_and(|meta| cfg_predicate_enables_test(&meta)))
    })
}

fn is_test_attribute_path(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "test" | "rstest" | "test_case" | "parameterized"
        )
    })
}

fn classify_source_file(path: &Path) -> FileProvenance {
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    let generated_path = components
        .iter()
        .any(|component| component == "generated" || component == "autogenerated");
    let vendored_path = components.iter().any(|component| {
        matches!(
            component.as_str(),
            "vendor" | "vendored" | "third_party" | "third-party"
        )
    });
    let generated_marker = fs::read_to_string(path).ok().is_some_and(|source| {
        let header = source
            .lines()
            .take(40)
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        [
            "@generated",
            "do not edit",
            "automatically generated",
            "auto-generated",
            "autogenerated",
            "code generated",
            "generated by",
        ]
        .iter()
        .any(|marker| header.contains(marker))
    });

    if generated_path || generated_marker {
        FileProvenance::Generated
    } else if vendored_path {
        FileProvenance::Vendored
    } else {
        FileProvenance::Project
    }
}

fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

fn has_external_representation_attr(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if attribute.path().is_ident("serde") || attribute.path().is_ident("diesel") {
            return true;
        }
        if !attribute.path().is_ident("derive") {
            return false;
        }
        let rendered = attribute.meta.to_token_stream().to_string();
        [
            "Serialize",
            "Deserialize",
            "Queryable",
            "Insertable",
            "AsChangeset",
            "FromSqlRow",
        ]
        .iter()
        .any(|name| {
            rendered
                .split(|ch: char| !ch.is_alphanumeric())
                .any(|token| token == *name)
        })
    })
}

fn return_type_names(signature: &Signature, self_type: Option<&str>) -> BTreeSet<String> {
    struct Collector<'a> {
        names: BTreeSet<String>,
        self_type: Option<&'a str>,
    }

    impl<'ast> Visit<'ast> for Collector<'_> {
        fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
            if let Some(segment) = path.path.segments.last() {
                let name = segment.ident.to_string();
                if name == "Self" {
                    if let Some(self_type) = self.self_type {
                        self.names.insert(self_type.to_string());
                    }
                } else {
                    self.names.insert(name);
                }
            }
            syn::visit::visit_type_path(self, path);
        }
    }

    let ReturnType::Type(_, ty) = &signature.output else {
        return BTreeSet::new();
    };
    let mut collector = Collector {
        names: BTreeSet::new(),
        self_type,
    };
    collector.visit_type(ty);
    collector.names
}

fn associated_path_owner(path: &syn::Path) -> Option<String> {
    if path.segments.len() < 2 {
        return None;
    }
    let owner = path.segments.iter().rev().nth(1)?.ident.to_string();
    (owner == "Self"
        || owner
            .chars()
            .next()
            .is_some_and(|first| first.is_uppercase()))
    .then_some(owner)
}

fn cfg_predicate_enables_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .is_ok_and(|nested| nested.iter().any(cfg_predicate_enables_test)),
        _ => false,
    }
}

fn is_test_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    normalized.contains("/tests/")
        || normalized.starts_with("tests/")
        || normalized.ends_with("/tests.rs")
        || normalized.ends_with("/test.rs")
        || normalized.ends_with("_tests.rs")
        || normalized.ends_with("_test.rs")
}

fn is_test_module_name(name: &str) -> bool {
    matches!(name, "test" | "tests") || name.ends_with("_test") || name.ends_with("_tests")
}

#[derive(Debug, Default)]
struct LocalTypeCatalog {
    all: HashSet<String>,
    string_replacements: HashSet<String>,
}

impl LocalTypeCatalog {
    fn from_project(project: &ProjectData) -> Self {
        let mut catalog = Self::default();
        for item in &project.enums {
            catalog.all.insert(item.name.clone());
            catalog.string_replacements.insert(item.name.clone());
        }
        for item in &project.structs {
            catalog.all.insert(item.name.clone());
            if item.fields.len() == 1
                && item.fields[0]
                    .split_once(':')
                    .is_some_and(|(_, ty)| is_text_storage_type(ty))
            {
                catalog.string_replacements.insert(item.name.clone());
            }
        }
        catalog.all.extend(
            project
                .type_decls
                .iter()
                .filter(|item| item.kind == "alias")
                .map(|item| item.name.clone()),
        );
        catalog
    }

    fn contains(&self, name: &str) -> bool {
        self.all.contains(name)
    }

    fn can_replace_string(&self, name: &str) -> bool {
        self.string_replacements.contains(name)
    }
}

fn is_text_storage_type(raw: &str) -> bool {
    let compact: String = raw.chars().filter(|ch| !ch.is_whitespace()).collect();
    matches!(
        compact.as_str(),
        "String"
            | "std::string::String"
            | "alloc::string::String"
            | "str"
            | "Box<str>"
            | "std::boxed::Box<str>"
            | "alloc::boxed::Box<str>"
            | "Arc<str>"
            | "std::sync::Arc<str>"
            | "Rc<str>"
            | "std::rc::Rc<str>"
            | "alloc::rc::Rc<str>"
    ) || compact.starts_with("Cow<") && compact.ends_with(",str>")
}

fn enum_name_for(site: &Site) -> String {
    let normalized = site.name.trim_start_matches("r#").to_ascii_lowercase();
    let context = site
        .callable_owner
        .as_deref()
        .unwrap_or_else(|| site.owner.rsplit("::").next().unwrap_or(&site.owner));
    let context = pascal_case(&enum_context_stem(context));
    let generic_name = normalized.chars().count() <= 2
        || matches!(
            normalized.as_str(),
            "str" | "string" | "value" | "name" | "key" | "short" | "input" | "raw" | "text"
        );
    let mut type_name = if generic_name {
        if matches!(normalized.as_str(), "id" | "key" | "value") {
            let context = contextual_type_stem(&context);
            let suffix = pascal_case(&normalized);
            if context.ends_with(&suffix) {
                context.to_string()
            } else {
                format!("{context}{suffix}")
            }
        } else {
            context.clone()
        }
    } else {
        pascal_case(&site.name)
    };
    if matches!(normalized.as_str(), "kind" | "type") {
        let suffix = pascal_case(&normalized);
        type_name = if context.ends_with(&suffix) {
            context
        } else {
            format!("{context}{suffix}")
        };
    }
    type_name
}

fn contextual_type_stem(context: &str) -> &str {
    ["Info", "Config"]
        .into_iter()
        .find_map(|suffix| context.strip_suffix(suffix).filter(|stem| !stem.is_empty()))
        .unwrap_or(context)
}

fn enum_context_stem(context: &str) -> String {
    let original = context.trim_start_matches("r#");
    if !original.contains('_') {
        return original.to_string();
    }
    let normalized = original.to_ascii_lowercase();
    let before_relation = ["_from_", "_for_", "_to_"]
        .into_iter()
        .filter_map(|marker| normalized.find(marker))
        .min()
        .map_or(normalized.as_str(), |position| &normalized[..position]);
    [
        "try_parse_",
        "parse_",
        "decode_",
        "deserialize_",
        "deserialise_",
        "coerce_",
        "convert_",
        "transform_",
        "map_",
        "from_",
    ]
    .into_iter()
    .find_map(|prefix| before_relation.strip_prefix(prefix))
    .unwrap_or(before_relation)
    .to_string()
}

fn borrowed_path_display(declared_type: &str) -> String {
    declared_type.trim().strip_suffix("str").map_or_else(
        || "&std::path::Path".to_string(),
        |prefix| format!("{prefix}std::path::Path"),
    )
}

fn pascal_case(name: &str) -> String {
    let mut output = String::new();
    for word in name
        .trim_start_matches("r#")
        .split(|ch: char| !ch.is_alphanumeric())
    {
        if word.is_empty() {
            continue;
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            output.extend(first.to_uppercase());
            output.push_str(chars.as_str());
        }
    }
    if output.is_empty() {
        "TextValue".to_string()
    } else {
        output
    }
}

fn normalize_type_path(raw: &str) -> String {
    normalize_token_spacing(raw)
        .trim_start_matches('&')
        .trim()
        .to_string()
}

fn normalize_token_spacing(raw: &str) -> String {
    raw.replace(" :: ", "::")
        .replace(" < ", "<")
        .replace(" >", ">")
        .replace("& ", "&")
        .trim()
        .to_string()
}

fn last_path_segment(path: &str) -> &str {
    path.rsplit("::")
        .next()
        .unwrap_or(path)
        .split('<')
        .next()
        .unwrap_or(path)
        .trim()
}

fn replace_path_root(path: &str, replacement: &str) -> String {
    match path.split_once("::") {
        Some((_, rest)) => format!("{}::{}", replacement, rest),
        None => replacement.to_string(),
    }
}

fn render_tokens(tokens: &impl ToTokens) -> String {
    normalize_token_spacing(&tokens.to_token_stream().to_string())
}

fn evidence_rank(kind: StringlyEvidenceKind) -> u8 {
    match kind {
        StringlyEvidenceKind::ExplicitConversion => 0,
        StringlyEvidenceKind::LiteralMatch => 1,
        StringlyEvidenceKind::LiteralComparison => 2,
        StringlyEvidenceKind::LiteralConstruction => 3,
        StringlyEvidenceKind::LiteralCallSite => 4,
        StringlyEvidenceKind::SharedVocabulary => 5,
        StringlyEvidenceKind::StructuredLiteral => 6,
        StringlyEvidenceKind::SemanticName => 7,
    }
}

fn caveat_rank(kind: StringlyCaveatKind) -> u8 {
    match kind {
        StringlyCaveatKind::GeneratedCode => 0,
        StringlyCaveatKind::VendoredCode => 1,
        StringlyCaveatKind::TraitContract => 2,
        StringlyCaveatKind::TextBoundary => 3,
        StringlyCaveatKind::ImplementationDetail => 4,
        StringlyCaveatKind::PublicApi => 5,
        StringlyCaveatKind::ExternalRepresentation => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn analyse_with_suppressed(
        source: &str,
        manifest: &str,
        include_suppressed: bool,
    ) -> StringlyReport {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("src")).expect("src");
        fs::write(dir.path().join("src/lib.rs"), source).expect("source");
        fs::write(dir.path().join("Cargo.toml"), manifest).expect("manifest");
        let project = ProjectData::load(dir.path(), false);
        detect_stringly(
            &project,
            &StringlyConfig {
                project_roots: vec![dir.path().to_path_buf()],
                min_confidence: 0.0,
                include_tests: true,
                include_suppressed,
            },
        )
    }

    fn analyse(source: &str, manifest: &str) -> StringlyReport {
        analyse_with_suppressed(source, manifest, true)
    }

    #[test]
    fn explicit_parse_beats_name_heuristics_and_resolves_import() {
        let report = analyse(
            "use std::net::IpAddr;\npub fn parse(host: String) { let _: IpAddr = host.parse().unwrap(); }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|item| item.name == "host")
            .unwrap();
        assert_eq!(finding.suggestion.canonical_path, "std::net::IpAddr");
        assert_eq!(finding.suggestion.origin, TypeOrigin::StandardLibrary);
        assert_eq!(finding.confidence, 0.99);
    }

    #[test]
    fn typed_free_parser_result_infers_the_success_type() {
        let report = analyse(
            "pub struct Config;\npub fn load(raw: String) { let _: Result<Config, ()> = parser::from_str(raw); }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|item| item.name == "raw")
            .unwrap();
        assert_eq!(finding.suggestion.canonical_path, "Config");
        assert_eq!(finding.suggestion.origin, TypeOrigin::LocalType);
        assert_eq!(finding.confidence, 0.99);
    }

    #[test]
    fn arbitrary_std_paths_and_os_strings_stay_in_standard_library_bucket() {
        let report = analyse(
            "pub fn count(raw: String) { let _ = raw.parse::<std::num::NonZeroU64>(); }\n\
             pub fn os(raw_os: String) { let _ = std::ffi::OsString::from(raw_os); }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        for name in ["raw", "raw_os"] {
            let finding = report
                .findings
                .iter()
                .find(|item| item.name == name)
                .unwrap();
            assert_eq!(finding.suggestion.origin, TypeOrigin::StandardLibrary);
            assert!(!finding.suggestion.new_dependency);
        }
        let nonzero = report
            .findings
            .iter()
            .find(|item| item.name == "raw")
            .unwrap();
        assert_eq!(nonzero.suggestion.canonical_path, "std::num::NonZeroU64");
    }

    #[test]
    fn conversion_confidence_requires_conversion_to_dominate_uses() {
        let report = analyse(
            "pub fn dominant(raw: String) { let _: u64 = raw.trim().parse().unwrap(); }\n\
             pub fn mixed(raw: String) { let _also_used = &raw; let _: u64 = raw.parse().unwrap(); }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let dominant = report
            .findings
            .iter()
            .find(|item| item.owner == "dominant")
            .unwrap();
        let mixed = report
            .findings
            .iter()
            .find(|item| item.owner == "mixed")
            .unwrap();
        assert_eq!(dominant.confidence, 0.99);
        assert_eq!(mixed.confidence, 0.82);
        assert!(
            mixed
                .evidence
                .iter()
                .any(|evidence| evidence.message.contains("does not dominate"))
        );
    }

    #[test]
    fn macro_arguments_count_as_non_conversion_uses() {
        let report = analyse(
            "pub fn mixed(raw: String) { println!(\"{}\", raw); let _: u64 = raw.parse().unwrap(); }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|item| item.name == "raw")
            .unwrap();
        assert_eq!(finding.confidence, 0.82);
    }

    #[test]
    fn field_evidence_uses_the_base_variables_type_instead_of_a_global_name_match() {
        let report = analyse(
            "pub struct Config { pub port: String }\n\
             pub struct External;\n\
             pub fn parse(config: &Config, external: &External) {\n\
                 let _: u16 = config.port.parse().unwrap();\n\
                 let _: std::net::IpAddr = external.port.parse().unwrap();\n\
             }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|item| item.name == "port")
            .unwrap();
        assert_eq!(finding.suggestion.canonical_path, "u16");
        assert_eq!(finding.confidence, 0.99);
        assert_eq!(
            finding
                .evidence
                .iter()
                .filter(|evidence| evidence.kind == StringlyEvidenceKind::ExplicitConversion)
                .count(),
            1
        );
    }

    #[test]
    fn finite_match_vocabulary_proposes_local_enum() {
        let report = analyse(
            r#"pub fn run(status: String) {
                match status.as_str() { "ready" => {}, "running" => {}, _ => {} }
            }"#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|item| item.name == "status")
            .unwrap();
        assert_eq!(finding.suggestion.display, "Status");
        assert_eq!(finding.suggestion.origin, TypeOrigin::LocalType);
        assert_eq!(finding.literals, ["ready", "running"]);
        assert_eq!(finding.confidence, 0.80);
    }

    #[test]
    fn literal_match_uses_an_existing_enum_returned_by_every_arm() {
        let report = analyse(
            r#"
            pub enum StorageBucket { UserFiles, SharedFiles }
            pub fn allowed(bucket: &str) -> bool {
                let parsed = match bucket {
                    "user_files" => StorageBucket::UserFiles,
                    "shared_files" => StorageBucket::SharedFiles,
                    _ => return false,
                };
                matches!(parsed, StorageBucket::UserFiles)
            }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.name == "bucket")
            .expect("bucket finding");
        assert_eq!(finding.suggestion.display, "StorageBucket");
        assert_eq!(finding.suggestion.origin, TypeOrigin::LocalType);
        assert_eq!(finding.confidence, 0.98);
        assert!(finding.evidence.iter().any(|evidence| {
            evidence
                .message
                .contains("existing nominal type StorageBucket")
        }));
    }

    #[test]
    fn associated_methods_are_not_assumed_to_return_their_owner_type() {
        let report = analyse(
            r#"
            pub struct PhoneNumbers;
            impl PhoneNumbers {
                fn active() -> Vec<u8> { vec![] }
                fn inactive() -> Vec<u8> { vec![] }
            }
            pub fn filter(status: &str) -> Vec<u8> {
                match status {
                    "active" => PhoneNumbers::active(),
                    "inactive" => PhoneNumbers::inactive(),
                    _ => vec![],
                }
            }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.name == "status")
            .expect("status vocabulary");
        assert_eq!(finding.suggestion.display, "Status");
        assert_eq!(finding.confidence, 0.80);
        assert!(finding.evidence.iter().all(|evidence| {
            !evidence
                .message
                .contains("existing nominal type PhoneNumbers")
        }));
    }

    #[test]
    fn constructor_style_static_methods_are_kept_when_the_signature_confirms_the_type() {
        let report = analyse(
            r#"
            pub struct CommitmentConfig;
            impl CommitmentConfig {
                fn processed() -> Self { Self }
                fn finalized() -> Self { Self }
            }
            pub fn parse(value: &str) -> Result<CommitmentConfig, ()> {
                match value {
                    "processed" => Ok(CommitmentConfig::processed()),
                    "finalized" => Ok(CommitmentConfig::finalized()),
                    _ => Err(()),
                }
            }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.name == "value")
            .expect("confirmed constructor target");
        assert_eq!(finding.suggestion.display, "CommitmentConfig");
        assert_eq!(finding.confidence, 0.98);
    }

    #[test]
    fn repeated_comparisons_do_not_turn_open_values_into_shared_enums() {
        let report = analyse(
            r#"
            fn first(host: &str) -> bool { host == "localhost" || host == "127.0.0.1" }
            fn second(host: &str) -> bool { host == "localhost" || host == "127.0.0.1" }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let hosts: Vec<_> = report
            .findings
            .iter()
            .filter(|finding| finding.name == "host")
            .collect();
        assert_eq!(hosts.len(), 2);
        assert!(hosts.iter().all(|finding| finding.confidence == 0.78));
        assert!(hosts.iter().all(|finding| {
            finding
                .evidence
                .iter()
                .all(|evidence| evidence.kind != StringlyEvidenceKind::SharedVocabulary)
        }));
    }

    #[test]
    fn rejecting_subset_match_is_downgraded_when_other_values_are_observed() {
        let report = analyse(
            r#"
            pub struct FunctionInfo { pub name: String }
            pub fn classify(function: &FunctionInfo) -> Option<u8> {
                match function.name.as_str() {
                    "from" => Some(1), "into" => Some(2), _ => None,
                }
            }
            pub fn make() -> FunctionInfo {
                FunctionInfo { name: "arbitrary_function".to_string() }
            }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.name == "name")
            .expect("lower-confidence audit finding");
        assert_eq!(finding.confidence, 0.80);
        assert!(
            finding
                .evidence
                .iter()
                .any(|evidence| { evidence.message.contains("partial classifier") })
        );
    }

    #[test]
    fn arbitrary_turbofish_calls_are_not_conversion_evidence() {
        let report = analyse(
            "fn consume<T>(_: String) {}\npub fn run(raw: String) { consume::<u64>(raw); }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        assert!(report.findings.iter().all(|finding| finding.name != "raw"));
    }

    #[test]
    fn free_and_method_literal_calls_use_separate_indexes() {
        let report = analyse(
            r#"
            pub fn set_mode(mode: String) {}
            pub struct Worker;
            impl Worker { pub fn set_mode(&self, input: String) {} }
            pub fn calls(worker: &Worker) {
                set_mode("free-a".into()); set_mode("free-b".into());
                worker.set_mode("method-a".into()); worker.set_mode("method-b".into());
            }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let free = report
            .findings
            .iter()
            .find(|finding| finding.owner == "set_mode")
            .expect("free function finding");
        let method = report
            .findings
            .iter()
            .find(|finding| finding.owner == "Worker::set_mode")
            .expect("method finding");
        assert_eq!(free.literals, ["free-a", "free-b"]);
        assert_eq!(method.literals, ["method-a", "method-b"]);
    }

    #[test]
    fn text_converters_and_trait_contracts_are_withheld_but_auditable() {
        let source = r#"
            pub fn year_bound(raw: &str) -> Option<i32> { raw.parse::<i32>().ok() }
            pub trait Decode { fn decode(&self, raw: &str) -> u64; }
            pub struct Decoder;
            impl Decode for Decoder {
                fn decode(&self, raw: &str) -> u64 { raw.parse::<u64>().unwrap() }
            }
        "#;
        let manifest = "[package]\nname='fixture'\nversion='0.1.0'\n";
        let actionable = analyse_with_suppressed(source, manifest, false);
        assert!(actionable.findings.is_empty());
        assert_eq!(actionable.suppressed_findings, 2);
        assert_eq!(actionable.suppression_counts.text_boundary, 2);
        assert_eq!(actionable.suppression_counts.trait_contract, 1);

        let audit = analyse_with_suppressed(source, manifest, true);
        assert_eq!(audit.findings.len(), 2);
        assert!(
            audit
                .findings
                .iter()
                .all(StringlyFinding::is_suppressed_by_default)
        );
    }

    #[test]
    fn parser_machinery_is_not_recommended_as_a_domain_value() {
        let source = r#"
            pub struct Parser;
            impl Parser { pub fn from_str(_: &str) -> Self { Self } }
            pub fn process(source: &str) { let _parser = Parser::from_str(source); }
        "#;
        let manifest = "[package]\nname='fixture'\nversion='0.1.0'\n";
        let actionable = analyse_with_suppressed(source, manifest, false);
        assert!(actionable.findings.is_empty());
        assert_eq!(actionable.suppression_counts.implementation_detail, 1);

        let audit = analyse_with_suppressed(source, manifest, true);
        let finding = audit
            .findings
            .iter()
            .find(|finding| finding.name == "source")
            .expect("auditable parser finding");
        assert!(finding.caveats.iter().any(|caveat| {
            caveat.kind == StringlyCaveatKind::ImplementationDetail && caveat.suppresses_by_default
        }));
    }

    #[test]
    fn ingestion_and_interop_representation_boundaries_are_withheld() {
        let source = r#"
            pub struct CaptureFile;
            impl CaptureFile { pub fn from_str(_: &str) -> Self { Self } }
            pub fn import_pairings(json_text: &str) {
                let _file = CaptureFile::from_str(json_text);
            }
            pub fn put(value: &str) {
                let _value = wasm_bindgen::JsValue::from_str(value);
            }
        "#;
        let manifest =
            "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\nwasm-bindgen='0.2'\n";
        let actionable = analyse_with_suppressed(source, manifest, false);
        assert!(actionable.findings.is_empty());
        assert_eq!(actionable.suppression_counts.text_boundary, 1);
        assert_eq!(actionable.suppression_counts.implementation_detail, 1);

        let audit = analyse_with_suppressed(source, manifest, true);
        assert_eq!(audit.findings.len(), 2);
    }

    #[test]
    fn generated_and_vendor_paths_are_classified_conservatively() {
        let dir = tempfile::tempdir().expect("tempdir");
        let generated = dir.path().join("src/generated/model.rs");
        let vendored = dir.path().join("vendor/lib.rs");
        let authored = dir.path().join("src/codegen.rs");
        assert_eq!(classify_source_file(&generated), FileProvenance::Generated);
        assert_eq!(classify_source_file(&vendored), FileProvenance::Vendored);
        assert_eq!(classify_source_file(&authored), FileProvenance::Project);
    }

    #[test]
    fn public_serialized_fields_keep_non_blocking_migration_caveats() {
        let report = analyse(
            "#[derive(Serialize)]\npub struct Request { pub callback_url: String }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report.findings.first().expect("semantic URL finding");
        assert!(!finding.is_suppressed_by_default());
        assert!(finding.caveats.iter().any(|caveat| {
            caveat.kind == StringlyCaveatKind::PublicApi && !caveat.suppresses_by_default
        }));
        assert!(finding.caveats.iter().any(|caveat| {
            caveat.kind == StringlyCaveatKind::ExternalRepresentation
                && !caveat.suppresses_by_default
        }));
    }

    #[test]
    fn renamed_existing_dependency_is_reported_without_new_dependency() {
        let report = analyse(
            "pub struct Request { pub callback_url: String }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\nweb-url={package='url',version='2'}\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|item| item.name == "callback_url")
            .unwrap();
        assert_eq!(finding.suggestion.display, "web_url::Url");
        assert_eq!(finding.suggestion.canonical_path, "url::Url");
        assert_eq!(finding.suggestion.origin, TypeOrigin::ExistingDependency);
        assert!(!finding.suggestion.new_dependency);
    }

    #[test]
    fn name_only_id_is_lower_confidence_local_newtype() {
        let report = analyse(
            "pub struct User { pub user_id: String }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|item| item.name == "user_id")
            .unwrap();
        assert_eq!(finding.suggestion.display, "UserId");
        assert_eq!(finding.confidence, 0.72);
    }

    #[test]
    fn borrowed_path_suggestion_preserves_the_reference_and_lifetime() {
        assert_eq!(borrowed_path_display("&str"), "&std::path::Path");
        assert_eq!(
            borrowed_path_display("&'request str"),
            "&'request std::path::Path"
        );

        let report = analyse(
            "use std::path::PathBuf;\npub fn normalize(path: &str) { let _ = PathBuf::from(path); }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.name == "path")
            .expect("path conversion finding");
        assert_eq!(finding.suggestion.canonical_path, "std::path::Path");
        assert_eq!(finding.suggestion.display, "&std::path::Path");
    }

    #[test]
    fn qualified_imports_beat_same_named_project_types_during_origin_classification() {
        let report = analyse(
            "mod local { pub struct Url; }\nuse url::Url;\npub fn parse(raw: String) { let _ = Url::parse(raw); }\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\nurl='2'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|item| item.name == "raw")
            .unwrap();
        assert_eq!(finding.suggestion.canonical_path, "url::Url");
        assert_eq!(finding.suggestion.origin, TypeOrigin::ExistingDependency);
    }

    #[test]
    fn detector_does_not_treat_arbitrary_types_named_string_or_str_as_text() {
        let report = analyse(
            "mod custom { pub struct String; pub struct str; }\n\
             pub fn fake(a: custom::String, b: &custom::str) {}\n\
             pub fn real(a: String, b: &str, c: std::string::String, d: &core::primitive::str) {}\n",
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        assert_eq!(report.scanned_string_sites, 4);
    }

    #[test]
    fn literal_evidence_is_unicode_safe_and_bounded() {
        assert_eq!(literal_preview("short"), "short");
        let long = "🦀".repeat(121);
        let preview = literal_preview(&long);
        assert_eq!(preview.chars().count(), 121);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn cfg_test_detection_rejects_negation_and_feature_substrings() {
        let test: syn::Attribute = syn::parse_quote!(#[cfg(test)]);
        let nested: syn::Attribute = syn::parse_quote!(#[cfg(all(unix, test))]);
        let negated: syn::Attribute = syn::parse_quote!(#[cfg(not(test))]);
        let latest: syn::Attribute = syn::parse_quote!(#[cfg(feature = "latest")]);
        let tokio_test: syn::Attribute = syn::parse_quote!(#[tokio::test]);
        let rstest: syn::Attribute = syn::parse_quote!(#[rstest::rstest]);
        assert!(has_test_attr(&[test]));
        assert!(has_test_attr(&[nested]));
        assert!(has_test_attr(&[tokio_test]));
        assert!(has_test_attr(&[rstest]));
        assert!(!has_test_attr(&[negated]));
        assert!(!has_test_attr(&[latest]));
    }

    #[test]
    fn conventional_test_file_and_module_names_are_detected() {
        assert!(is_test_path("src/codex_tests.rs"));
        assert!(is_test_path("src/query_test.rs"));
        assert!(!is_test_path("src/contest.rs"));
        assert!(is_test_module_name("protocol_tests"));
        assert!(is_test_module_name("query_test"));
        assert!(!is_test_module_name("latest"));
    }

    #[test]
    fn deserialized_project_data_is_parsed_once_as_a_cache_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("src")).expect("src");
        let source_path = dir.path().join("src/lib.rs");
        fs::write(
            &source_path,
            "pub fn parse(raw: String) { let _ = raw.parse::<u64>(); }\n",
        )
        .expect("source");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )
        .expect("manifest");

        let loaded = ProjectData::load(dir.path(), false);
        let serialized = serde_json::to_string(&loaded).expect("serialize project");
        let restored: ProjectData = serde_json::from_str(&serialized).expect("restore project");
        assert!(restored.parsed_file(&source_path).is_none());

        let report = detect_stringly(
            &restored,
            &StringlyConfig {
                project_roots: vec![dir.path().to_path_buf()],
                min_confidence: 0.85,
                include_tests: true,
                include_suppressed: true,
            },
        );
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].suggestion.canonical_path, "u64");
    }

    #[test]
    fn excluded_tests_do_not_contribute_literal_evidence_to_production_sites() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("src")).expect("src");
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"
                pub fn set_mode(mode: &str) { let _ = mode; }
                #[cfg(test)]
                mod tests {
                    use super::set_mode;
                    #[test]
                    fn modes() { set_mode("fast"); set_mode("slow"); }
                }
            "#,
        )
        .expect("source");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )
        .expect("manifest");
        let project = ProjectData::load(dir.path(), false);
        let run = |include_tests| {
            detect_stringly(
                &project,
                &StringlyConfig {
                    project_roots: vec![dir.path().to_path_buf()],
                    min_confidence: 0.0,
                    include_tests,
                    include_suppressed: true,
                },
            )
        };

        assert!(
            run(true)
                .findings
                .iter()
                .any(|finding| finding.owner == "set_mode")
        );
        assert!(
            run(false)
                .findings
                .iter()
                .all(|finding| finding.owner != "set_mode")
        );
    }

    #[test]
    fn repeated_vocabulary_across_declarations_is_high_confidence_enum_evidence() {
        let report = analyse(
            r#"
                fn head(kind: &str) {}
                fn scalar(kind: &str) {}
                fn per_table(kind: &str) {}
                fn calls() {
                    head("counter"); head("gauge");
                    scalar("counter"); scalar("gauge");
                    per_table("counter"); per_table("gauge");
                }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let kinds: Vec<_> = report
            .findings
            .iter()
            .filter(|finding| finding.name == "kind")
            .collect();
        assert_eq!(kinds.len(), 3);
        assert!(kinds.iter().all(|finding| {
            finding.suggestion.display == "Kind"
                && finding.confidence == 0.93
                && finding
                    .literal_stats
                    .as_ref()
                    .is_some_and(|stats| stats.distinct == 2 && stats.observations == 2)
        }));
        assert!(kinds.iter().all(|finding| {
            finding
                .evidence
                .iter()
                .any(|evidence| evidence.kind == StringlyEvidenceKind::SharedVocabulary)
        }));
    }

    #[test]
    fn large_repeated_label_spaces_do_not_get_the_small_enum_boost() {
        let calls = (0..20)
            .flat_map(|index| {
                [
                    format!("lookup(\"key_{index}\");"),
                    format!("lookup(\"key_{index}\");"),
                ]
            })
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!("fn lookup(key: &str) {{}}\nfn calls() {{ {calls} }}");
        let report = analyse(&source, "[package]\nname='fixture'\nversion='0.1.0'\n");
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.name == "key")
            .expect("lower-confidence audit finding");
        assert_eq!(finding.literal_stats.as_ref().unwrap().distinct, 20);
        assert_eq!(finding.confidence, 0.78);
    }

    #[test]
    fn protocol_literals_choose_existing_http_type_and_local_endpoint_enum() {
        let report = analyse(
            r#"
                fn respond(status: &str) { let _ = format!("{status}"); }
                fn build(path: &str) {}
                fn calls() {
                    respond("200 OK"); respond("404 Not Found");
                    build("/v1/queries");
                    build("/v1/subscriptions");
                    build("/v1/transactions");
                }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n[dependencies]\nhttp='1'\n",
        );
        let status = report
            .findings
            .iter()
            .find(|finding| finding.name == "status")
            .expect("status finding");
        assert_eq!(status.suggestion.canonical_path, "http::StatusCode");
        assert_eq!(status.suggestion.origin, TypeOrigin::ExistingDependency);
        assert_eq!(status.confidence, 0.96);
        assert!(!status.is_suppressed_by_default());

        let path = report
            .findings
            .iter()
            .find(|finding| finding.name == "path")
            .expect("endpoint finding");
        assert_eq!(path.suggestion.display, "Endpoint");
        assert_eq!(path.suggestion.origin, TypeOrigin::LocalType);
        assert_eq!(path.confidence, 0.90);
    }

    #[test]
    fn absolute_filesystem_paths_are_not_mistaken_for_http_endpoints() {
        let report = analyse(
            r#"
                fn open(path: &str) {}
                fn calls() { open("/tmp/input"); open("/etc/example.conf"); }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let path = report
            .findings
            .iter()
            .find(|finding| finding.name == "path")
            .expect("semantic path finding");
        assert_eq!(path.suggestion.canonical_path, "std::path::Path");
        assert_ne!(path.suggestion.display, "Endpoint");
    }

    #[test]
    fn local_type_exists_requires_an_enum_or_text_wrapper_shape() {
        let report = analyse(
            r#"
                struct Instance { value: String, healthy: bool }
                struct AppId(String);
                enum Mode { Fast, Slow }
                fn route(instance: &str) {}
                fn lookup(app_id: &str) {}
                fn set(mode: &str) {}
                fn calls() {
                    route("one"); route("two");
                    set("fast"); set("slow");
                }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let instance = report
            .findings
            .iter()
            .find(|finding| finding.name == "instance")
            .expect("instance finding");
        assert_eq!(instance.suggestion.display, "Instance");
        assert!(!instance.suggestion.local_type_exists);

        let app_id = report
            .findings
            .iter()
            .find(|finding| finding.name == "app_id")
            .expect("app id finding");
        assert!(app_id.suggestion.local_type_exists);

        let mode = report
            .findings
            .iter()
            .find(|finding| finding.name == "mode")
            .expect("mode finding");
        assert!(mode.suggestion.local_type_exists);
    }

    #[test]
    fn prose_and_one_off_labels_are_text_boundaries() {
        let report = analyse(
            r#"
                fn metric(help: &str, name: &str) {
                    println!("{help}: {name}");
                }
                fn calls() {
                    metric("live connections", "edge_live");
                    metric("candidate machines", "edge_candidates");
                }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        for name in ["help", "name"] {
            let finding = report
                .findings
                .iter()
                .find(|finding| finding.name == name)
                .expect("auditable label finding");
            assert!(finding.caveats.iter().any(|caveat| {
                caveat.kind == StringlyCaveatKind::TextBoundary && caveat.suppresses_by_default
            }));
        }
    }

    #[test]
    fn diagnostic_text_boundary_propagates_through_parameter_forwarding() {
        let report = analyse(
            r#"
                enum ParseError<'a> { Bad(&'a str) }
                fn fail(what: &str) { let _ = ParseError::Bad(what); }
                fn wrapper(context: &str) { fail(context); }
                fn calls() { wrapper("eof"); wrapper("token"); }
            "#,
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.name == "context")
            .expect("auditable forwarded diagnostic finding");
        assert!(finding.caveats.iter().any(|caveat| {
            caveat.kind == StringlyCaveatKind::TextBoundary && caveat.message.contains("flows only")
        }));
    }
}
