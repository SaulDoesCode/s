{a 1 b 2 c 3 d 4 e 5 f 6 g 7 h 8 i 9 j 10 k 11 l 12 m 13 n 14 o 15 p 16 q 17 r 18 s 19 t 20 u 21 v 22 w 23 x 24 y 25 z 26} ord`
{letters i~ ch [l]  ord~ {~ch~} 0 ?)  sum~ sum +  i;  i~ len~ <<  letter..?} letter$
{{~word~} letters {} /l  letters len #l  0 i#  0 sum#  letter.  {~word~ ~sum~} >} gem$
{args j~ word [l]  gem.  j;  j~ argc~ <<  each..?} each$
{{gematria memory base} args /l  args argc #l} defaults$
argc~ 0 =  defaults.?
0 j#  each.
