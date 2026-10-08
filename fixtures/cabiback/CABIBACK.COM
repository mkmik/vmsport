$! ANALYZE/RMS_FILE/CHECK and DCL READ on files vmsport's C ABI wrote.
$ SET NOON
$ i = 0
$next:
$ f = F$ELEMENT(i, ",", "DEMO.REL,DEMO.IDX,DEMO.SEQ")
$ IF f .EQS. "," THEN EXIT 1
$ WRITE SYS$OUTPUT "@@ check ", f
$ ANALYZE/RMS_FILE/CHECK/OUTPUT=CHECK.TMP 'f'
$ SEARCH CHECK.TMP "analysis uncovered"
$ WRITE SYS$OUTPUT "@@ read ", f
$ OPEN/READ in 'f'
$read:
$ READ/END_OF_FILE=done in r
$ WRITE SYS$OUTPUT "[", r, "]"
$ GOTO read
$done:
$ CLOSE in
$ i = i + 1
$ GOTO next
