$! Substitution and verb symbols.
$ SET NOON
$ n = 3
$ s = "abc"
$ WRITE SYS$OUTPUT "n='n' s=''s' both ''n'''s'"
$ x'n' = "made x3"
$ WRITE SYS$OUTPUT x3
$ name = "S"
$ WRITE SYS$OUTPUT 'name'
$ WRITE SYS$OUTPUT &name
$ w :== WRITE SYS$OUTPUT
$ w "via a verb symbol"
$ say*it :== WRITE SYS$OUTPUT
$ say "abbreviated verb"
$ sayit "full verb"
$ WRITE SYS$OUTPUT "'F$LENGTH(s)' and ''F$LENGTH(s)'"
$ e = "1 + 2"
$ v = 'e'
$ WRITE SYS$OUTPUT v
$ q = "a""b"
$ WRITE SYS$OUTPUT q, " ", F$LENGTH(q)
$ WRITE SYS$OUTPUT "undefined ['nosuch'] [''nosuch']"
$ t := "  spaced   out  "
$ WRITE SYS$OUTPUT "[", t, "]"
$ t := unquoted   words  here
$ WRITE SYS$OUTPUT "[", t, "]"
$ g == "global"
$ CALL showg
$ EXIT
$showg: SUBROUTINE
$ g = "local"
$ WRITE SYS$OUTPUT g
$ SHOW SYMBOL g
$ SHOW SYMBOL/GLOBAL g
$ DELETE/SYMBOL g
$ WRITE SYS$OUTPUT g
$ ENDSUBROUTINE
