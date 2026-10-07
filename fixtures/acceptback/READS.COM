$! Every record of P1 in the order of each of its P2 keys: from the
$! first, found with a GE match on a value before them all (an empty key
$! is RMS-F-KSZ). Key 2 of ORDERS descends.
$ SET NOON
$ k = 0
$key:
$ WRITE SYS$OUTPUT "@@ ", P1, " key ", k
$ OPEN/READ f 'P1'
$ start = " "
$ IF k .EQ. 2 THEN start = "~"
$ READ/INDEX='k'/KEY="''start'"/MATCH=GE/ERROR=done f rec
$next:
$ WRITE SYS$OUTPUT rec
$ READ/END_OF_FILE=done f rec
$ GOTO next
$done:
$ SHOW SYMBOL $STATUS
$ CLOSE f
$ k = k + 1
$ IF k .LT. P2 THEN GOTO key
