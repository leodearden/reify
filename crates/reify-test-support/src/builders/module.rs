use reify_compiler::{
    CompiledField, CompiledImport, CompiledModule, CompiledPurpose, CompiledTrait, TopologyTemplate,
};
use reify_types::{ContentHash, SourceSpan};

/// Builder for `CompiledModule`.
pub struct CompiledModuleBuilder {
    path: reify_types::ModulePath,
    imports: Vec<CompiledImport>,
    functions: Vec<reify_types::CompiledFunction>,
    trait_defs: Vec<CompiledTrait>,
    templates: Vec<TopologyTemplate>,
    diagnostics: Vec<reify_types::Diagnostic>,
    fields: Vec<CompiledField>,
    enum_defs: Vec<reify_types::EnumDef>,
    compiled_purposes: Vec<CompiledPurpose>,
}

impl CompiledModuleBuilder {
    pub fn new(path: reify_types::ModulePath) -> Self {
        Self {
            path,
            imports: Vec::new(),
            functions: Vec::new(),
            trait_defs: Vec::new(),
            templates: Vec::new(),
            diagnostics: Vec::new(),
            fields: Vec::new(),
            enum_defs: Vec::new(),
            compiled_purposes: Vec::new(),
        }
    }

    pub fn trait_def(mut self, t: CompiledTrait) -> Self {
        self.trait_defs.push(t);
        self
    }

    pub fn function(mut self, f: reify_types::CompiledFunction) -> Self {
        self.functions.push(f);
        self
    }

    pub fn import(mut self, path: impl Into<String>) -> Self {
        self.imports.push(CompiledImport {
            path: path.into(),
            kind: reify_syntax::ImportKind::Module,
            is_pub: false,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn import_with(
        mut self,
        path: impl Into<String>,
        kind: reify_syntax::ImportKind,
        is_pub: bool,
    ) -> Self {
        self.imports.push(CompiledImport {
            path: path.into(),
            kind,
            is_pub,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn template(mut self, template: TopologyTemplate) -> Self {
        self.templates.push(template);
        self
    }

    pub fn diagnostic(mut self, diag: reify_types::Diagnostic) -> Self {
        self.diagnostics.push(diag);
        self
    }

    pub fn field(mut self, f: CompiledField) -> Self {
        self.fields.push(f);
        self
    }

    pub fn enum_def(mut self, e: reify_types::EnumDef) -> Self {
        self.enum_defs.push(e);
        self
    }

    pub fn compiled_purpose(mut self, p: CompiledPurpose) -> Self {
        self.compiled_purposes.push(p);
        self
    }

    pub fn build(self) -> CompiledModule {
        // Build a content-sensitive hash matching compile() logic.
        let content_hash = {
            let path_hash = ContentHash::of_str(&format!("{}", self.path));

            let template_hashes = self.templates.iter().map(|t| t.content_hash);

            let import_hashes = self.imports.iter().map(|i| ContentHash::of_str(&i.path));

            let function_hashes = self.functions.iter().map(|f| f.content_hash);

            let trait_def_hashes = self.trait_defs.iter().map(|t| t.content_hash);

            let field_hashes = self.fields.iter().map(|f| f.content_hash);

            let purpose_hashes = self.compiled_purposes.iter().map(|p| p.content_hash);

            let enum_hashes = self.enum_defs.iter().map(|e| ContentHash::of_str(&e.name));

            let all_hashes = std::iter::once(path_hash)
                .chain(template_hashes)
                .chain(import_hashes)
                .chain(function_hashes)
                .chain(trait_def_hashes)
                .chain(field_hashes)
                .chain(purpose_hashes)
                .chain(enum_hashes);

            ContentHash::combine_all(all_hashes)
        };

        CompiledModule {
            path: self.path,
            imports: self.imports,
            enum_defs: self.enum_defs,
            functions: self.functions,
            trait_defs: self.trait_defs,
            fields: self.fields,
            compiled_purposes: self.compiled_purposes,
            templates: self.templates,
            units: Vec::new(),
            type_aliases: Vec::new(),
            constraint_defs: Vec::new(),
            pragmas: Vec::new(),
            default_tolerance: None,
            declared_version: None,
            solver_pragma: None,
            kernel_pragma: None,
            diagnostics: self.diagnostics,
            content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builders::{
        CompiledFieldBuilder, CompiledPurposeBuilder, CompiledTraitBuilder, TraitDefBuilder,
        literal,
    };
    use reify_types::{EnumDef, ModulePath, Type, Value};

    fn module_path() -> ModulePath {
        ModulePath::new(vec!["test".to_string()])
    }

    #[test]
    fn module_builder_with_trait_def() {
        let t = CompiledTraitBuilder::new("Rigid")
            .require_param("thickness", Type::length())
            .build();
        let module = CompiledModuleBuilder::new(module_path())
            .trait_def(t)
            .build();
        assert_eq!(module.trait_defs.len(), 1);
        assert_eq!(module.trait_defs[0].name, "Rigid");
    }

    #[test]
    fn module_builder_trait_defs_affect_content_hash() {
        let module_no_traits = CompiledModuleBuilder::new(module_path()).build();
        let ct = TraitDefBuilder::new("Rigid").build();
        let module_with_trait = CompiledModuleBuilder::new(module_path())
            .trait_def(ct)
            .build();
        assert_ne!(
            module_no_traits.content_hash, module_with_trait.content_hash,
            "modules differing only in trait_defs must produce distinct content_hashes"
        );
    }

    #[test]
    fn module_builder_with_field() {
        let body = literal(Value::Real(1.0));
        let f = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .analytical(body)
            .build();
        let module = CompiledModuleBuilder::new(module_path()).field(f).build();
        assert_eq!(module.fields.len(), 1);
        assert_eq!(module.fields[0].name, "temp");
    }

    #[test]
    fn module_builder_with_enum_def() {
        let e = EnumDef {
            name: "Color".to_string(),
            variants: vec!["Red".to_string(), "Blue".to_string()],
        };
        let module = CompiledModuleBuilder::new(module_path())
            .enum_def(e)
            .build();
        assert_eq!(module.enum_defs.len(), 1);
        assert_eq!(module.enum_defs[0].name, "Color");
        assert_eq!(module.enum_defs[0].variants.len(), 2);
    }

    #[test]
    fn module_builder_with_compiled_purpose() {
        let p = CompiledPurposeBuilder::new("mfg_ready")
            .param("subject", "Structure")
            .build();
        let module = CompiledModuleBuilder::new(module_path())
            .compiled_purpose(p)
            .build();
        assert_eq!(module.compiled_purposes.len(), 1);
        assert_eq!(module.compiled_purposes[0].name, "mfg_ready");
    }

    #[test]
    fn module_builder_hash_changes_with_new_fields() {
        let empty_module = CompiledModuleBuilder::new(module_path()).build();
        let t = CompiledTraitBuilder::new("Rigid").build();
        let with_trait = CompiledModuleBuilder::new(module_path())
            .trait_def(t)
            .build();
        assert_ne!(empty_module.content_hash, with_trait.content_hash);
    }
}
