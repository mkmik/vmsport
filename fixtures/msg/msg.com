$! Compiles TESTMSG.MSG with the VMS MESSAGE utility and prints every message
$! it defines through F$MESSAGE, plus how DCL shows statuses.
$ SET NOON
$ MESSAGE/LIST=TESTMSG.LIS/SDL=TESTMSG.SDL TESTMSG.MSG
$ LINK/SHAREABLE=TESTMSG.EXE TESTMSG.OBJ
$ SET MESSAGE DKA200:[T]TESTMSG.EXE
$ fac = 3282
$ GOSUB scan
$ fac = 77
$ GOSUB scan
$ fac = 4095
$ GOSUB scan
$! Severity letter: from the code or from the message definition?
$ sev = 0
$sevloop:
$ code = 3282 * %X10000 + %X8000 + 2 * 8 + sev
$ WRITE SYS$OUTPUT "sev ", sev, " ", F$MESSAGE(code)
$ sev = sev + 1
$ IF sev .LE. 7 THEN GOTO sevloop
$! Components
$ code = 3282 * %X10000 + %X8000 + 2 * 8 + 0
$ WRITE SYS$OUTPUT "comp ", F$MESSAGE(code, "TEXT")
$ WRITE SYS$OUTPUT "comp ", F$MESSAGE(code, "IDENT")
$ WRITE SYS$OUTPUT "comp ", F$MESSAGE(code, "SEVERITY")
$ WRITE SYS$OUTPUT "comp ", F$MESSAGE(code, "FACILITY")
$ WRITE SYS$OUTPUT "comp ", F$MESSAGE(code, "FACILITY,IDENT")
$ WRITE SYS$OUTPUT "comp ", F$MESSAGE(code, "SEVERITY,TEXT")
$! Messages that are not there
$ WRITE SYS$OUTPUT "none ", F$MESSAGE(3282 * %X10000 + %X8000 + 50 * 8 + 2)
$ WRITE SYS$OUTPUT "none ", F$MESSAGE(%X12345678)
$ WRITE SYS$OUTPUT "none ", F$MESSAGE(0)
$ WRITE SYS$OUTPUT "none ", F$MESSAGE(%X0FFFFFFF)
$! System messages
$ WRITE SYS$OUTPUT "sys ", F$MESSAGE(1)
$ WRITE SYS$OUTPUT "sys ", F$MESSAGE(%X2C)
$ WRITE SYS$OUTPUT "sys ", F$MESSAGE(%X651)
$ WRITE SYS$OUTPUT "sys ", F$MESSAGE(%X18292)
$ WRITE SYS$OUTPUT "sys ", F$MESSAGE(%X38240)
$! How DCL shows a status from EXIT
$ @EXITWITH 'F$STRING(3282 * %X10000 + %X8000 + 1 * 8 + 3)'
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ @EXITWITH 'F$STRING(3282 * %X10000 + %X8000 + 2 * 8 + 0)'
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ @EXITWITH 'F$STRING(3282 * %X10000 + %X8000 + 3 * 8 + 2)'
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ @EXITWITH 'F$STRING(3282 * %X10000 + %X8000 + 100 * 8 + 4)'
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ @EXITWITH 'F$STRING(%X10000000 + 3282 * %X10000 + %X8000 + 3 * 8 + 2)'
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ @EXITWITH 'F$STRING(3282 * %X10000 + %X8000 + 50 * 8 + 2)'
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ @EXITWITH 44
$ WRITE SYS$OUTPUT "status ", F$FAO("!XL", F$INTEGER($STATUS))
$ SET MESSAGE/NOFACILITY
$ @EXITWITH 44
$ SET MESSAGE/NOIDENTIFICATION/FACILITY
$ @EXITWITH 44
$ SET MESSAGE/NOSEVERITY/IDENTIFICATION
$ @EXITWITH 44
$ SET MESSAGE/NOTEXT/SEVERITY
$ @EXITWITH 44
$ SET MESSAGE/TEXT
$ EXIT 1
$!
$scan:
$ n = 0
$scanloop:
$ fs = 0
$fsloop:
$ code = fac * %X10000 + fs + n * 8
$ msg = F$MESSAGE(code)
$ IF F$LOCATE("-NOMSG,", msg) .EQ. F$LENGTH(msg) THEN -
	WRITE SYS$OUTPUT "msg ", F$FAO("!XL", code), " ", msg
$ fs = fs + %X8000
$ IF fs .LE. %X8000 THEN GOTO fsloop
$ n = n + 1
$ IF n .LE. 120 THEN GOTO scanloop
$ RETURN
