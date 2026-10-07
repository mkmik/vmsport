$! The records of the acceptance files, from a number: P1 "ORDER" or
$! "PART", P2 the number. Leaves the record in the global symbol REC.
$ custs = "ACME,GLOBEX,INITECH,UMBRELLA,HOOLI,STARK,WAYNE,TYRELL,CYBERDYNE,SOYLENT,MASSIVE,VANDELAY,WONKA"
$ desc = "widgets gadgets gizmos sprockets doohickeys thingamajigs whatsits"
$ k = F$INTEGER(P2)
$ IF P1 .EQS. "PART" THEN GOTO part
$ cust = F$ELEMENT(k - (k / 13) * 13, ",", custs)
$ amt = k * 53 - (k * 53 / 10000) * 10000
$ REC == F$FAO("O!7ZL!10AS!4ZL", k, cust, amt) + F$EXTRACT(0, (k - (k / 6) * 6) * 10, desc)
$ EXIT
$part:
$ fam = k - (k / 9) * 9
$ loc = k - (k / 7) * 7
$ REC == F$FAO("F!3ZLLOC!3ZL!4ZL!26AS", fam, loc, k, F$FAO("part number !5ZL", k))
$ EXIT
