$ SET NOON
$ say = "WRITE SYS$OUTPUT"
$ OPEN/READ f RB.DAT
$ say "@@ rb"
$ READ f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID000050" f r
$ st = $STATUS
$ GOSUB show
$ READ f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID00005"/MATCH=GE f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID00005" f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID00005"/MATCH=GT f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID000010" f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID000010"/MATCH=GE f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID000010"/MATCH=GT f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID000149"/MATCH=GT f r
$ st = $STATUS
$ GOSUB show
$ READ f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="ID000151"/MATCH=GE f r
$ st = $STATUS
$ GOSUB show
$ READ/INDEX=1/KEY="OSLO" f r
$ st = $STATUS
$ GOSUB show
$ n = 0
$l1:
$ READ f r
$ st = $STATUS
$ GOSUB show
$ n = n + 1
$ IF n .LT. 3 THEN GOTO l1
$ READ/INDEX=1/KEY="P"/MATCH=GE f r
$ st = $STATUS
$ GOSUB show
$ READ/INDEX=1/KEY="P"/MATCH=GT f r
$ st = $STATUS
$ GOSUB show
$ READ/INDEX=1/KEY="NEWCITY" f r
$ st = $STATUS
$ GOSUB show
$ READ/INDEX=1/KEY="ZAGREB" f r
$ st = $STATUS
$ GOSUB show
$ n = 0
$l2:
$ READ f r
$ st = $STATUS
$ GOSUB show
$ n = n + 1
$ IF n .LT. 22 THEN GOTO l2
$ READ/INDEX=2/KEY="0001" f r
$ st = $STATUS
$ GOSUB show
$ READ f r
$ st = $STATUS
$ GOSUB show
$ READ/INDEX=3/KEY="N050" f r
$ st = $STATUS
$ GOSUB show
$ READ f r
$ st = $STATUS
$ GOSUB show
$ READ f r
$ st = $STATUS
$ GOSUB show
$ CLOSE f
$!
$ OPEN/READ f KS.DAT
$ say "@@ ks"
$ READ/INDEX=1/KEY="P"/MATCH=GE f r
$ st = $STATUS
$ GOSUB show
$ READ/INDEX=1/KEY="P"/MATCH=GT f r
$ st = $STATUS
$ GOSUB show
$ READ/INDEX=1/KEY="ZZ"/MATCH=GE f r
$ st = $STATUS
$ GOSUB show
$ READ/INDEX=1/KEY="A"/MATCH=GE f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="A0" f r
$ st = $STATUS
$ GOSUB show
$ CLOSE f
$!
$ OPEN/READ f PD.DAT
$ say "@@ pd"
$ READ/KEY="AAA" f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="BBB" f r
$ st = $STATUS
$ GOSUB show
$ READ/KEY="AAA"/MATCH=GT f r
$ st = $STATUS
$ GOSUB show
$ CLOSE f
$ EXIT 1
$!
$show:
$ IF st THEN say "+ ", r
$ IF .NOT. st THEN say "- ", F$FAO("!XL", st)
$ RETURN
