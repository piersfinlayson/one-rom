// Shipping tray for cased One ROMs. Use make-tray.sh, or set these here.
count = 25;
cols = 0;           // 0 chooses the grid
rows = 0;
part = "base";      // "base" or "top"
bed = [250, 210];   // Original Prusa MK3

// Width, length, and width and length of the narrow end
cap = [26, 28, 20, 8];      // ../cases/cap-with-header-pins.stl
case_end = [23, 25, 17, 8]; // ../cases/case-*-with-header-pins.stl

clearance = 0.3;
wall = 1.2;
border = 5;
rim_h = 10;
floor_t = 1;
depth = 3;
lip = 2;
tab = [12, 7];      // along the edge, out from the edge
slot = [5.5, 2];

pitch = [cap[0], cap[1]] + [1, 1] * (2 * clearance + wall);
height = floor_t + depth;

function size(c, r) = [c * pitch.x, r * pitch.y] + [1, 1] * (2 * border - wall);
function fits(c, r) = let(s = size(c, r)) s.x + 2 * tab.y <= bed.x && s.y + 2 * tab.y <= bed.y;

// Fewest pockets, then the squarest grid, at most 3 times longer than wide
function better(a, b) = b == undef || a[0] < b[0] || (a[0] == b[0] && a[1] - a[2] < b[1] - b[2]);
function pick(l, i = 0, b = undef) = i == len(l) ? b : pick(l, i + 1, better(l[i], b) ? l[i] : b);
choices = [for (r = [1:count]) let(c = ceil(count / r)) if (c >= r && c <= 3 * r && fits(c, r)) [c * r, c, r]];
auto = pick(choices);

assert(cols > 0 || auto != undef, str(count, " units don't fit on one plate"));
c = cols > 0 ? cols : auto[1];
r = rows > 0 ? rows : auto[2];
assert(c * r >= count, str(c, "x", r, " holds fewer than ", count, " units"));
assert(fits(c, r), str(c, "x", r, " doesn't fit the bed"));
echo(grid = str(c, "x", r));

s = size(c, r);
shape = part == "base" ? cap : case_end;

module tabs() {
    for (p = [[s.x / 2, -tab.y / 2, 0], [s.x / 2, s.y + tab.y / 2, 180],
              [-tab.y / 2, s.y / 2, -90], [s.x + tab.y / 2, s.y / 2, 90]])
        translate([p[0], p[1]]) rotate(p[2]) difference() {
            translate([-tab.x / 2, -tab.y / 2]) cube([tab.x, tab.y + 1, height]);
            translate([-slot.x / 2, -slot.y / 2, -1]) cube([slot.x, slot.y, height + 2]);
        }
}

module at_pockets() {
    for (i = [0:c - 1], j = [0:r - 1])
        translate([border + i * pitch.x + pitch.x / 2 - wall / 2,
                   border + j * pitch.y + pitch.y / 2 - wall / 2]) children();
}

module outline() {
    translate([-shape[0] / 2, -shape[1] / 2]) square([shape[0], shape[1] - shape[3]]);
    translate([-shape[2] / 2, shape[1] / 2 - shape[3]]) square([shape[2], shape[3]]);
}

difference() {
    union() {
        cube([s.x, s.y, height]);
        difference() {
            cube([s.x, s.y, rim_h]);
            translate([border, border, -1]) cube([s.x - 2 * border, s.y - 2 * border, rim_h + 2]);
        }
        tabs();
    }
    at_pockets() {
        translate([0, 0, floor_t]) linear_extrude(depth + 1) offset(delta = clearance) outline();
        translate([0, 0, -1]) linear_extrude(height + 2) offset(delta = clearance - lip) outline();
    }
}
