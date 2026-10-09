{values stack up from the left; the word at the right acts on them} //
hello from s >
6 7 *  =~ >

{a script body is a brace literal; lookups in it run when it is called} //
{n~ fact~ fact *  n:  n~ 1 >>  step..?} step$
5 n#  1 fact#  step.
{5! = ~fact~} >

{a call in tail position loops in constant space} //
{i~ >_ { } >_  i;  i~ 10 <<  count..?} count$
>  0 i#  count.

{conditions set ow; ? and ! run a word only when ow is true or false} //
3 4 <<  smaller? larger! >

{lists, split into characters, and a table lookup} //
{a 1 b 2 c 3 d 4 e 5} small`
{dabbed} cs {} /l
{cs i~ ch [l]  small~ {~ch~} 0 ?)  sum~ sum +  i;  i~ len~ <<  add..?} add$
cs len #l  0 i#  0 sum#  add.
{dabbed sums to ~sum~} >
