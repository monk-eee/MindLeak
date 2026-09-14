- The ADR publication guard now reports the first unstaged modification or
  deletion of a decision record. Shared Git reads preserve leading status
  columns instead of trimming away the information needed to parse the path.
