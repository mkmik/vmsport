$ nested_local = 1
$ nested_global == "set by nested"
$ WRITE SYS$OUTPUT "nested ", P1, " ", P2, " depth ", F$ENVIRONMENT("DEPTH")
$ EXIT %X10000003
