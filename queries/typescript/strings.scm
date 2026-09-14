(string) @string
(template_string) @string

(import_statement source: (string) @skip)
(export_statement source: (string) @skip)
(call_expression
  function: (identifier) @_fn
  arguments: (arguments (string) @skip)
  (#eq? @_fn "require"))
(call_expression function: (import) arguments: (arguments (string) @skip))

(pair key: (string) @skip)
(program (expression_statement (string) @skip))
(property_signature name: (string) @skip)
(public_field_definition name: (string) @skip)
(method_definition name: (string) @skip)
(enum_assignment name: (string) @skip)
(literal_type (string) @skip)
(subscript_expression index: (string) @skip)
