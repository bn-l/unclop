; `.declarator` captures are followed down the declarator chain to the name.

(function_definition declarator: (_) @fn.declarator)
(declaration declarator: (_) @var.declarator)
(parameter_declaration declarator: (_) @param.declarator)
(field_declaration declarator: (_) @field.declarator)
(type_definition declarator: (_) @type.declarator)

(struct_specifier name: (type_identifier) @type body: (_))
(union_specifier name: (type_identifier) @type body: (_))
(enum_specifier name: (type_identifier) @type body: (_))
(enumerator name: (identifier) @variant)

(preproc_def name: (identifier) @macro)
(preproc_function_def name: (identifier) @macro)
