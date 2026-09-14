(function_declaration name: (identifier) @fn)
(generator_function_declaration name: (identifier) @fn)
(function_signature name: (identifier) @fn)
(function_expression name: (identifier) @fn)
(method_definition name: (_) @fn)
(method_signature name: (_) @fn)
(abstract_method_signature name: (_) @fn)

(class_declaration name: (type_identifier) @type)
(abstract_class_declaration name: (type_identifier) @type)
(class name: (type_identifier) @type)
(interface_declaration name: (type_identifier) @type)
(type_alias_declaration name: (type_identifier) @type)
(enum_declaration name: (identifier) @type)

(enum_body (property_identifier) @variant)
(enum_assignment name: (_) @variant)

(public_field_definition name: (_) @field)
(property_signature name: (_) @field)

(variable_declarator name: (_) @var.pattern)

(required_parameter pattern: (_) @param.pattern)
(optional_parameter pattern: (_) @param.pattern)
(arrow_function parameter: (identifier) @param)

(catch_clause parameter: (_) @var.pattern)
(for_in_statement left: (_) @var.pattern)

(internal_module name: (identifier) @mod)
