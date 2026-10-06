$! What SET VERIFY shows.
$ SET NOON
$ WRITE SYS$OUTPUT "verify ", F$VERIFY()
$ SET VERIFY
$ a = 1 ! a comment
$ b = "x"
$ IF a THEN -
	WRITE SYS$OUTPUT "continued line"
$label:
$ WRITE SYS$OUTPUT 'a'
$ old = F$VERIFY(0)
$ WRITE SYS$OUTPUT "was ", old
$ WRITE SYS$OUTPUT "now ", F$VERIFY()
$ SET VERIFY
$ SET NOVERIFY
$ WRITE SYS$OUTPUT "off"
$ EXIT
