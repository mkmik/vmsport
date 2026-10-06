$! Prints every message VMS has for the SYSTEM (0, shared), RMS (1) and
$! CLI (3) facilities: code and F$MESSAGE text.
$ SET NOON
$ CALL scan 0 0
$ CALL scan 1 %X8000
$ CALL scan 3 %X8000
$ EXIT 1
$scan: SUBROUTINE
$ fac = F$INTEGER(P1)
$ fs = F$INTEGER(P2)
$ n = 0
$loop:
$ code = fac * %X10000 + fs + n * 8
$ msg = F$MESSAGE(code)
$ IF F$LOCATE("-NOMSG,", msg) .EQ. F$LENGTH(msg) THEN -
	WRITE SYS$OUTPUT F$FAO("!XL", code), " ", msg
$ n = n + 1
$ IF n .LT. 4096 THEN GOTO loop
$ ENDSUBROUTINE
