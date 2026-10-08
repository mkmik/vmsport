$! PIPE: pipelines, sequences, conditions, redirection.
$ SET NOON
$ OPEN/WRITE f WORDS.TXT
$ WRITE f "alpha"
$ WRITE f "beta"
$ WRITE f "gamma"
$ CLOSE f
$ WRITE SYS$OUTPUT "@@ pipe search"
$ PIPE TYPE WORDS.TXT | SEARCH SYS$PIPE a
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ pipe write"
$ PIPE WRITE SYS$OUTPUT "one two" | SEARCH SYS$PIPE two
$ WRITE SYS$OUTPUT "@@ pipe sequence keeps symbols?"
$ PIPE y = 5 ; SHOW SYMBOL y
$ SHOW SYMBOL y
$ WRITE SYS$OUTPUT "@@ pipe and"
$ PIPE WRITE SYS$OUTPUT "first" && WRITE SYS$OUTPUT "second"
$ PIPE nosuchverb && WRITE SYS$OUTPUT "not shown"
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ pipe or"
$ PIPE nosuchverb || WRITE SYS$OUTPUT "or branch"
$ PIPE WRITE SYS$OUTPUT "ok" || WRITE SYS$OUTPUT "not shown"
$ WRITE SYS$OUTPUT "@@ pipe redirect out"
$ PIPE SHOW SYMBOL $STATUS > PIPEOUT.TXT
$ TYPE PIPEOUT.TXT
$ WRITE SYS$OUTPUT "@@ pipe redirect in"
$ PIPE SEARCH SYS$INPUT beta < WORDS.TXT
$ WRITE SYS$OUTPUT "@@ pipe subshell"
$ PIPE (WRITE SYS$OUTPUT "a1" ; WRITE SYS$OUTPUT "b1") | SEARCH SYS$PIPE 1
$ WRITE SYS$OUTPUT "@@ pipe status of last"
$ PIPE WRITE SYS$OUTPUT "x" | SEARCH SYS$PIPE nomatch
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ pipe read"
$ PIPE WRITE SYS$OUTPUT "piped text" | (READ SYS$PIPE line ; WRITE SYS$OUTPUT "got [", line, "]")
$ WRITE SYS$OUTPUT "@@ pipe error redirect"
$ PIPE nosuchverb 2> PIPEERR.TXT
$ TYPE PIPEERR.TXT
$ WRITE SYS$OUTPUT "@@ pipe three"
$ PIPE TYPE WORDS.TXT | SEARCH SYS$PIPE a | SEARCH SYS$PIPE m
$ EXIT
