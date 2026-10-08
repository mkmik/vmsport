$! HELP over TEST.HLB (made from TEST.HLP): layout, lookups, prompts.
$ SET NOON
$ LIBRARY/CREATE/HELP TEST.HLB TEST.HLP
$ lib = "DKA200:[T.HELP]TEST.HLB"
$ CALL h "top" ""
$ CALL h "topic" "ALPHA"
$ CALL h "abbreviation" "AL"
$ CALL h "ambiguous" "BE"
$ CALL h "exact beats longer" "BETA"
$ CALL h "one letter" "E"
$ CALL h "subtopic" "ALPHA SUBTOPIC_ONE"
$ CALL h "level three" "ALPHA SUBTOPIC_ONE DEEPER"
$ CALL h "qualifier" "ALPHA/LOG"
$ CALL h "qualifier spaced" "ALPHA /COUNT"
$ CALL h "two qualifiers" "ALPHA/LOG/CONFIRM"
$ CALL h "qualifiers topic" "ALPHA QUALIFIERS"
$ CALL h "parameters" "ALPHA PARAMETERS"
$ CALL h "wildcard top" "*"
$ CALL h "wildcard sub" "ALPHA *"
$ CALL h "percent" "B%TA"
$ CALL h "ellipsis" "DELTA..."
$ CALL h "ellipsis missing" "NOSUCH..."
$ CALL h "no such topic" "NOSUCH"
$ CALL h "no such subtopic" "ALPHA NOSUCH"
$ CALL h "two topics" "BETA GAMMA"
$ CALL h "two topics found" "BETA DELTA"
$ CALL h "qualifier abbreviation" "ALPHA/LO"
$ CALL h "negated qualifier" "ALPHA/NOLOG"
$ CALL h "no text" "NOTEXT"
$ CALL h "long name" "GAMMA_LONGNAME"
$ CALL h "one letter exact" "E"
$ CALL h "ellipsis found" "ALPHA..."
$ CALL h "help topic" "HELP"
$ CALL h "qualifier wildcard" "ALPHA/*"
$ CALL h "lowercase" "alpha subtopic_two"
$ WRITE SYS$OUTPUT "@@ noinstructions"
$ HELP/LIBRARY='lib'/NOPROMPT/NOINSTRUCTIONS
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ prompting"
$ HELP/LIBRARY='lib'/PROMPT
ALPHA
SUBTOPIC_ONE
?
DEEPER


NOSUCH
DELTA
ONE



$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ prompting from a topic"
$ HELP/LIBRARY='lib'/PROMPT BETA
GAMMA

$ WRITE SYS$OUTPUT "@@ output file"
$ HELP/LIBRARY='lib'/NOPROMPT/OUTPUT=HELP.LIS BETA
$ TYPE HELP.LIS
$ WRITE SYS$OUTPUT "@@ no library"
$ HELP/LIBRARY=NOSUCH.HLB/NOPROMPT BETA
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ EXIT
$h: SUBROUTINE
$ WRITE SYS$OUTPUT "@@ ", P1
$ HELP/LIBRARY='lib'/NOPROMPT 'P2'
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ ENDSUBROUTINE
