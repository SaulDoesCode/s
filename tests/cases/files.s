tmp_files.txt {line one} w
tmp_files.txt {
line two} +w
tmp_files.txt r  tmp_files.txt~ >
tmp_files.txt 5 3 part r@  part~ >
tmp_files.txt h {r} f
h l1 _f  l1~ >  h ?eof  eof~ >
h l2 _f  l2~ >  h ?eof  eof~ >
h l3 _f  {[~l3~]} >  h ?eof  eof~ >
h f-
tmp_files2.txt h2 {w} f
h2 first f_  h2 second f_  h2 f-
tmp_files2.txt r  tmp_files2.txt~ >
{XY} patch`
tmp_files2.txt h3 r+ f  h3 0 patch +f  h3 0 4 got @f  got~ >  h3 f-
tmp_files2.txt tmp_files3.txt mv
tmp_files3.txt r  tmp_files3.txt~ >
missing_dir/x.txt hz {r} {err~ >  could not open >} f
{echo shell; exit 3} <  r~ rc~ >
{rm tmp_files.txt tmp_files3.txt} <
