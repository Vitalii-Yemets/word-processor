use strict;
use warnings;

my @new = (
    ['Always Open Read-Only', 'Immer schreibgeschützt öffnen'],
    ['Always open read-only', 'Immer schreibgeschützt öffnen'],
    ['File sharing', 'Dateifreigabe'],
    ['General Options', 'Allgemeine Optionen'],
    ['Open read-only', 'Schreibgeschützt öffnen'],
    ['Opened read-only', 'Schreibgeschützt geöffnet'],
    ['Password to modify', 'Kennwort zum Ändern'],
    ['Password to modify (optional)', 'Kennwort zum Ändern (optional)'],
    ['Read-Only Recommended', 'Schreibschutz empfohlen'],
    ['The author asked for it to be opened read-only.', 'Der Autor möchte, dass es schreibgeschützt geöffnet wird.'],
    ['The two passwords are not the same', 'Die beiden Kennwörter stimmen nicht überein'],
    ['This document is open read-only.', 'Dieses Dokument ist schreibgeschützt geöffnet.'],
    ['This document was opened read-only: save a copy', 'Dieses Dokument wurde schreibgeschützt geöffnet: Kopie speichern'],
    ['{0} is reserved.', '{0} ist reserviert.'],
);

undef $/;
my $text = <STDIN>;

for my $pair (@new) {
    my ($key, $said) = @$pair;
    next if $text =~ /^\Q= $key\E$/m;
    my $block = "= $key\n> $said\n\n";
    my $put = 0;
    while ($text =~ /^= (.+)$/mg) {
        my $found = $1;
        if (lc($found) gt lc($key)) {
            my $at = pos($text) - length("= $found");
            $text = substr($text, 0, $at) . $block . substr($text, $at);
            $put = 1;
            last;
        }
    }
    $text .= $block unless $put;
}

print $text;
