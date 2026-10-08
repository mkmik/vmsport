$! READ/INDEX=P2/KEY=P3 in file P1: "+ FILE INDEX KEY" and the record's
$! first 16 bytes, or "- FILE INDEX KEY" if VMS found none.
$ SET NOON
$ OPEN/READ f 'P1'
$ READ/INDEX='P2'/KEY="''P3'" f r
$ st = $STATUS
$ IF st THEN WRITE SYS$OUTPUT "+ ", P1, " ", P2, " ", P3, " ", F$EXTRACT(0, 16, r)
$ IF .NOT. st THEN WRITE SYS$OUTPUT "- ", P1, " ", P2, " ", P3
$ CLOSE f
