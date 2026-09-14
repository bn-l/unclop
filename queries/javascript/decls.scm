(function_declaration name: (identifier) @fn)
(generator_function_declaration name: (identifier) @fn)
(function_expression name: (identifier) @fn)
(method_definition name: (_) @fn)

(class_declaration name: (identifier) @type)
(class name: (identifier) @type)

(field_definition property: (_) @field)

(variable_declarator name: (_) @var.pattern)

(formal_parameters (identifier) @param)
(formal_parameters (assignment_pattern left: (_) @param.pattern))
(formal_parameters (rest_pattern (identifier) @param))
(formal_parameters (object_pattern) @param.pattern)
(formal_parameters (array_pattern) @param.pattern)
(arrow_function parameter: (identifier) @param)

(catch_clause parameter: (_) @var.pattern)
(for_in_statement left: (_) @var.pattern)
