(function_definition name: (identifier) @fn)
(class_definition name: (identifier) @type)

(parameters (identifier) @param)
(parameters (default_parameter name: (identifier) @param))
(parameters (typed_parameter (identifier) @param))
(parameters (typed_parameter (list_splat_pattern (identifier) @param)))
(parameters (typed_parameter (dictionary_splat_pattern (identifier) @param)))
(parameters (typed_default_parameter name: (identifier) @param))
(parameters (list_splat_pattern (identifier) @param))
(parameters (dictionary_splat_pattern (identifier) @param))
(lambda_parameters (identifier) @param)
(lambda_parameters (default_parameter name: (identifier) @param))

(class_definition
  body: (block (expression_statement (assignment left: (identifier) @field))))

(assignment left: (_) @var.pattern)
(assignment
  left: (attribute object: (identifier) @_self attribute: (identifier) @field)
  (#eq? @_self "self"))

(for_statement left: (_) @var.pattern)
(as_pattern alias: (as_pattern_target (identifier) @var))
(named_expression name: (identifier) @var)
