$! Nested IF blocks, GOSUB, CALL and ON.
$ SET NOON
$ x = 2
$ IF x .GT. 1
$ THEN
$   IF x .EQ. 2
$   THEN
$     WRITE SYS$OUTPUT "two"
$   ELSE
$     WRITE SYS$OUTPUT "not two"
$   ENDIF
$   WRITE SYS$OUTPUT "big"
$ ELSE
$   IF x .EQ. 0 THEN WRITE SYS$OUTPUT "zero"
$   WRITE SYS$OUTPUT "small"
$ ENDIF
$ IF x .LT. 0 THEN WRITE SYS$OUTPUT "negative"
$ IF x
$ THEN
$   WRITE SYS$OUTPUT "x is odd"
$ ENDIF
$ IF .NOT. x THEN WRITE SYS$OUTPUT "x is even"
$ i = 0
$again:
$ i = i + 1
$ IF i .LT. 5
$ THEN
$   IF i .EQ. 3 THEN GOTO out
$   GOTO again
$ ENDIF
$out:
$ WRITE SYS$OUTPUT "left at ", i
$ GOSUB one
$ WRITE SYS$OUTPUT "gosub status ", F$FAO("!XL", F$INTEGER($STATUS))
$ CALL fact 5
$ WRITE SYS$OUTPUT "5! = ", result
$ CALL fails
$ WRITE SYS$OUTPUT "call status ", F$FAO("!XL", F$INTEGER($STATUS)), " sev ", $SEVERITY
$ CALL sub_on
$ WRITE SYS$OUTPUT "after sub_on ", F$FAO("!XL", F$INTEGER($STATUS))
$ WRITE SYS$OUTPUT "on severity ", F$ENVIRONMENT("ON_SEVERITY")
$ SET ON
$ WRITE SYS$OUTPUT "on severity ", F$ENVIRONMENT("ON_SEVERITY")
$ ON WARNING THEN CONTINUE
$ WRITE SYS$OUTPUT "on severity ", F$ENVIRONMENT("ON_SEVERITY")
$ nosuchverb
$ WRITE SYS$OUTPUT "continued ", F$FAO("!XL", F$INTEGER($STATUS))
$ SET NOON
$ RETURN
$ WRITE SYS$OUTPUT "bad return ", F$FAO("!XL", F$INTEGER($STATUS))
$ EXIT 1
$one:
$ WRITE SYS$OUTPUT "in one, depth ", F$ENVIRONMENT("DEPTH")
$ GOSUB two
$ RETURN %X1234
$two:
$ WRITE SYS$OUTPUT "in two"
$ RETURN
$fact: SUBROUTINE
$ IF P1 .LE. 1
$ THEN
$   result == 1
$   EXIT
$ ENDIF
$ CALL fact 'P1 - 1'
$ result == result * P1
$ EXIT
$ ENDSUBROUTINE
$fails: SUBROUTINE
$ WRITE SYS$OUTPUT "in fails, depth ", F$ENVIRONMENT("DEPTH")
$ EXIT %X2C
$ ENDSUBROUTINE
$sub_on: SUBROUTINE
$ ON ERROR THEN GOTO handler
$ x = F$CVTIME("bogus")
$ WRITE SYS$OUTPUT "not reached"
$handler:
$ WRITE SYS$OUTPUT "handled ", F$FAO("!XL", F$INTEGER($STATUS))
$ ENDSUBROUTINE
