; Declaration sites. Capture names are kinds; `.pattern` captures are walked for bindings.

(function_item name: (identifier) @fn)
(function_signature_item name: (identifier) @fn)

(struct_item name: (type_identifier) @type)
(enum_item name: (type_identifier) @type)
(union_item name: (type_identifier) @type)
(trait_item name: (type_identifier) @type)
(type_item name: (type_identifier) @type)

(enum_variant name: (identifier) @variant)
(field_declaration name: (field_identifier) @field)

(const_item name: (identifier) @const)
(static_item name: (identifier) @const)

(mod_item name: (identifier) @mod)
(macro_definition name: (identifier) @macro)

(let_declaration pattern: (_) @var.pattern)
(for_expression pattern: (_) @var.pattern)

(parameter pattern: (_) @param.pattern)
(closure_parameters (identifier) @param)
(closure_parameters (tuple_pattern) @param.pattern)
(closure_parameters (mut_pattern) @param.pattern)
(closure_parameters (ref_pattern) @param.pattern)
(closure_parameters (struct_pattern) @param.pattern)
(closure_parameters (tuple_struct_pattern) @param.pattern)
