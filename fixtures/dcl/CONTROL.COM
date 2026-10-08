$! Control flow, $STATUS, ON, CALL, GOSUB and nested procedures.
$ ON WARNING THEN GOTO err
$ WRITE SYS$OUTPUT "start ", $STATUS
$ i = 0
$loop:
$ i = i + 1
$ IF i .LT. 3
$ THEN
$   WRITE SYS$OUTPUT "loop ", i
$   GOTO loop
$ ELSE
$   WRITE SYS$OUTPUT "done ", i
$ ENDIF
$ GOSUB sub
$ WRITE SYS$OUTPUT "after gosub ", $STATUS
$ CALL proc one "two words" 3
$ WRITE SYS$OUTPUT "after call ", $STATUS, " ", $SEVERITY
$ @NESTED.COM alpha beta
$ WRITE SYS$OUTPUT "after nested ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "local in nested: ", F$TYPE(nested_local), -
	" global: ", nested_global
$ SET NOON
$ x = F$INTEGER("1") / 0
$ WRITE SYS$OUTPUT "divide ", F$FAO("!XL", F$INTEGER($STATUS))
$ nosuchverb
$ WRITE SYS$OUTPUT "bad verb ", F$FAO("!XL", F$INTEGER($STATUS)), " ", $SEVERITY
$ SET ON
$ ON ERROR THEN GOTO err2
$ WRITE SYS$OUTPUT "warning continues"
$ EXIT 16
$ WRITE SYS$OUTPUT "not reached"
$err2:
$ WRITE SYS$OUTPUT "err2 ", F$FAO("!XL", F$INTEGER($STATUS))
$ EXIT 3
$err:
$ WRITE SYS$OUTPUT "err ", F$FAO("!XL", F$INTEGER($STATUS))
$ EXIT
$sub:
$ WRITE SYS$OUTPUT "in sub"
$ RETURN 3
$proc: SUBROUTINE
$ WRITE SYS$OUTPUT "proc [", P1, "] [", P2, "] [", P3, "] [", P4, "]"
$ EXIT 9
$ ENDSUBROUTINE
