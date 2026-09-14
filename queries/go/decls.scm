(function_declaration name: (identifier) @fn)
(method_declaration name: (field_identifier) @fn)

(type_spec name: (type_identifier) @type)
(type_alias name: (type_identifier) @type)

(field_declaration name: (field_identifier) @field)

(const_spec name: (identifier) @const)
(var_spec name: (identifier) @var)
(short_var_declaration left: (expression_list (identifier) @var))
(range_clause left: (expression_list (identifier) @var))

(parameter_declaration name: (identifier) @param)
(variadic_parameter_declaration name: (identifier) @param)
