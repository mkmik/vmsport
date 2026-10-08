$! Errors DCL reports, each run with ON turned off.
$ SET NOON
$ WRITE SYS$OUTPUT "@@ undefined symbol"
$ WRITE SYS$OUTPUT nosuch
$ WRITE SYS$OUTPUT "@@ bad lexical"
$ x = F$NOSUCH(1)
$ WRITE SYS$OUTPUT "@@ wrong type"
$ x = F$LENGTH(1, 2)
$ WRITE SYS$OUTPUT "@@ unterminated"
$ x = "abc
$ WRITE SYS$OUTPUT "@@ invalid qualifier"
$ SHOW SYMBOL/NOSUCH x
$ WRITE SYS$OUTPUT "@@ ambiguous verb"
$ SE
$ WRITE SYS$OUTPUT "@@ extract error"
$ x = F$EXTRACT(-1, 2, "abc")
$ WRITE SYS$OUTPUT "@@ status ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "@@ goto missing"
$ GOTO nolabel
