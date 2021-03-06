
export class userCss {
	base16Colors: string[];

	constructor(base16Colors: string[]) {
		this.base16Colors = base16Colors;
	}

	darkest() { return this.base16Colors[0] }
	lightest() { return this.base16Colors[7] }
	red() { return this.base16Colors[8] }
	orange() { return this.base16Colors[9] }
	yellow() { return this.base16Colors[10] }
	green() { return this.base16Colors[11] }
	cyan() { return this.base16Colors[12] }
	blue() { return this.base16Colors[13] }
	violet() { return this.base16Colors[14] }
	magenta() { return this.base16Colors[15] }
}

// sulphurpool 
export const sulphurpool = new userCss([
    "hsl(229, 37%, 20%)",    /* #202746 */
    "hsl(229, 35%, 25%)",    /* #293256 */
    "hsl(228, 18%, 45%)",    /* #5e6687 */
    "hsl(229, 16%, 50%)",    /* #6b7394 */
    "hsl(229, 13%, 59%)",    /* #898ea4 */
    "hsl(229, 16%, 65%)",    /* #979db4 */
    "hsl(229, 40%, 91%)",    /* #dfe2f1 */
    "hsl(229, 94%, 98%)",    /* #f5f7ff */
    "hsl(14, 71%, 47%)",    /* #c94922 */
    "hsl(25, 66%, 47%)",     /* #c76b29 */
    "hsl(38, 60%, 47%)",     /* #c08b30 */
    "hsl(49, 50%, 45%)",     /* #ac9739 */
    "hsl(194, 71%, 46%)",    /* #22a2c9 */
    "hsl(207, 62%, 53%)",    /* #3d8fd1 */
    "hsl(229, 50%, 60%)",    /* #6679cc */
    "hsl(336, 22%, 50%)",    /* #9c637a */
]);

export const savanna = new userCss([
    "hsl(140, 10%, 10%)",  /* #171c19 */
    "hsl(140, 9%, 15%)",  /* #232a25 */
    "hsl(140, 8%, 35%)",  /* #526057 */
    "hsl(140, 7%, 40%)",  /* #5f6d64 */
    "hsl(140, 6%, 50%)",  /* #78877d */
    "hsl(140, 5%, 55%)",  /* #87928a */
    "hsl(140, 15%, 89%)",  /* #dfe7e2 */
    "hsl(140, 25%, 94%)",  /* #ecf4ee */
    "hsl(20, 51%, 46%)",  /* #b16139 */
    "hsl(32, 45%, 43%)",  /* #9f713c */
    "hsl(40, 46%, 43%)",  /* #a07e3b */
    "hsl(140, 36%, 44%)",  /* #489963 */
    "hsl(183, 70%, 37%)",  /* #1c9aa0 */
    "hsl(183, 34%, 42%)",  /* #478c90 */
    "hsl(199, 29%, 47%)",  /* #55859b */
    "hsl(21, 12%, 47%) ",  /* #867469 */
]);

export let defaultUserCss = sulphurpool;

