open my $fh, '<', '.tmpwork/j10.md' or die "j10: $!";
my $new = do { local $/; <$fh> };
close $fh;
undef $/;
my $text = <STDIN>;
$text =~ s{^- \[ \] \*\*J10\. The passwords.*?\n(?=- \[ \] \*\*J11\.)}{$new}ms or die "no J10";
print $text;
