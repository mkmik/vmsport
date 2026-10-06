$! SPAWN: what a subprocess gets, what it prints, what comes back.
$ SET NOON
$ x = 1
$ g == "global"
$ DEFINE VPT_LNM "a logical"
$ WRITE SYS$OUTPUT "@@ spawn write"
$ SPAWN WRITE SYS$OUTPUT "in subprocess"
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ spawn symbols"
$ SPAWN SHOW SYMBOL x
$ SPAWN SHOW SYMBOL g
$ WRITE SYS$OUTPUT "@@ spawn changes don't come back"
$ SPAWN x = 2
$ SHOW SYMBOL x
$ WRITE SYS$OUTPUT "@@ spawn logical"
$ SPAWN WRITE SYS$OUTPUT F$TRNLNM("VPT_LNM")
$ WRITE SYS$OUTPUT "@@ spawn exit status"
$ SPAWN EXIT 44
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ SPAWN EXIT %X10000002
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ spawn bad verb"
$ SPAWN nosuchverb
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ spawn nosymbol"
$ SPAWN/NOSYMBOL SHOW SYMBOL x
$ WRITE SYS$OUTPUT "@@ spawn nolog"
$ SPAWN/NOLOGICAL_NAMES WRITE SYS$OUTPUT "[", F$TRNLNM("VPT_LNM"), "]"
$ WRITE SYS$OUTPUT "@@ spawn default"
$ SPAWN WRITE SYS$OUTPUT F$ENVIRONMENT("DEFAULT")
$ WRITE SYS$OUTPUT "@@ spawn output"
$ SPAWN/OUTPUT=SPOUT.TXT WRITE SYS$OUTPUT "to a file"
$ TYPE SPOUT.TXT
$ WRITE SYS$OUTPUT "@@ spawn depth"
$ SPAWN WRITE SYS$OUTPUT F$ENVIRONMENT("DEPTH"), " ", F$MODE()
$ EXIT
