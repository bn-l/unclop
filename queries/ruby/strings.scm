(string) @string
(heredoc_body) @string

(call
  method: (identifier) @_m
  arguments: (argument_list (string) @skip)
  (#match? @_m "^(require|require_relative|load|gem|source|autoload)$"))
