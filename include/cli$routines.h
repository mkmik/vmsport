/* cli$routines.h: the command line interface routines (vmsport). A
 * table is CLD text: vmsport cdu makes it a C array, which is passed as
 * cli$dcl_parse(0, &NAME_TABLES). A null command parses the command DCL
 * ran the program with, or its argv when run from a Unix shell. */
#ifndef __CLI$ROUTINES_LOADED
#define __CLI$ROUTINES_LOADED 1
#include "vms_args.h"

unsigned int cli$present(void *entity_desc);
unsigned int cli$get_value(void *entity_desc, void *retdesc, unsigned short *retlen);
unsigned int cli$dcl_parse(void *command_string, void *table, void *param_routine, void *prompt_routine,
                           void *prompt_string);

#define cli$get_value(...) (cli$get_value)(_VMS_A3(__VA_ARGS__))
#define cli$dcl_parse(...) (cli$dcl_parse)(_VMS_A5(__VA_ARGS__))

#endif
