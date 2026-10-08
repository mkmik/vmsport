$! What OpenVMS makes of the indexed files SORT and CONVERT made here
$! (ours/): ANALYZE/RMS_FILE/CHECK, and their records in every key's order.
$ SET NOON
$ i = 0
$ana:
$ f = F$ELEMENT(i, ",", "C10,C11,C12,C13,C14,C15,C16,C19,C21,S21")
$ IF f .EQS. "," THEN GOTO done
$ ANALYZE/RMS_FILE/CHECK/OUTPUT='f'.CHK 'f'.DAT
$ WRITE SYS$OUTPUT "@@ ", f
$ n = F$FILE_ATTRIBUTES(f + ".DAT", "NOK")
$ k = 0
$keys:
$ IF k .GE. n THEN GOTO next
$ WRITE SYS$OUTPUT "@ key ", k
$ OPEN/READ in 'f'.DAT
$ READ/INDEX='k'/KEY=" "/MATCH=GE/END_OF_FILE=eof in r
$loop:
$ WRITE SYS$OUTPUT r
$ READ/END_OF_FILE=eof in r
$ GOTO loop
$eof:
$ CLOSE in
$ k = k + 1
$ GOTO keys
$next:
$ i = i + 1
$ GOTO ana
$done:
$ WRITE SYS$OUTPUT "@@ end"
