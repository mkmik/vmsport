$! OPEN, WRITE, READ, CLOSE, F$SEARCH and F$PARSE on files it makes.
$ SET NOON
$ OPEN/WRITE out NOTES.TXT
$ WRITE out "first line"
$ WRITE out "second, with ""quotes"""
$ WRITE out ""
$ WRITE out "last"
$ CLOSE out
$ OPEN/READ in NOTES.TXT
$ n = 0
$loop:
$ READ/END_OF_FILE=done in line
$ n = n + 1
$ WRITE SYS$OUTPUT n, ": [", line, "] ", F$LENGTH(line)
$ GOTO loop
$done:
$ WRITE SYS$OUTPUT "eof ", F$FAO("!XL", F$INTEGER($STATUS))
$ CLOSE in
$ OPEN/APPEND out NOTES.TXT
$ WRITE out "appended"
$ CLOSE out
$ OPEN/WRITE out NOTES.TXT
$ WRITE out "version 2"
$ CLOSE out
$ OPEN/WRITE out OTHER.DAT
$ CLOSE out
$ f = F$SEARCH("NOTES.TXT;*")
$ WRITE SYS$OUTPUT "search ", F$PARSE(f,,,"NAME"), F$PARSE(f,,,"TYPE"), F$PARSE(f,,,"VERSION")
$ f = F$SEARCH("NOTES.TXT;*")
$ WRITE SYS$OUTPUT "search ", F$PARSE(f,,,"NAME"), F$PARSE(f,,,"TYPE"), F$PARSE(f,,,"VERSION")
$ f = F$SEARCH("NOTES.TXT;*")
$ WRITE SYS$OUTPUT "search end [", f, "]"
$wild:
$ f = F$SEARCH("*.*")
$ IF f .EQS. "" THEN GOTO wilddone
$ name = F$PARSE(f,,,"NAME") + F$PARSE(f,,,"TYPE")
$ IF name .EQS. "NOTES.TXT" .OR. name .EQS. "OTHER.DAT" THEN -
	WRITE SYS$OUTPUT "wild ", name, F$PARSE(f,,,"VERSION")
$ GOTO wild
$wilddone:
$ OPEN/READ in NOTES.TXT
$ READ in line
$ WRITE SYS$OUTPUT "latest [", line, "]"
$ CLOSE in
$ OPEN/READ in NOTES.TXT;1
$ READ in line
$ READ in line
$ READ in line
$ READ in line
$ READ in line
$ WRITE SYS$OUTPUT "v1 fifth [", line, "]"
$ READ in line
$ WRITE SYS$OUTPUT "past end ", F$FAO("!XL", F$INTEGER($STATUS))
$ CLOSE in
$ OPEN/READ in NOSUCH.TXT
$ WRITE SYS$OUTPUT "open missing ", F$FAO("!XL", F$INTEGER($STATUS))
$ OPEN/READ/ERROR=nofile in NOSUCH.TXT
$ WRITE SYS$OUTPUT "not here"
$nofile:
$ WRITE SYS$OUTPUT "error label ", F$FAO("!XL", F$INTEGER($STATUS))
$ CLOSE nosuch
$ WRITE SYS$OUTPUT "close unopened ", F$FAO("!XL", F$INTEGER($STATUS))
$ READ nosuch line
$ EXIT
