//! Compile-time inventory of the pinned 1965 MAD programs.

/// One named MAD module embedded from the 1965 program inventory.
pub(crate) struct MadProgram {
    /// Inventory-relative module name.
    pub(crate) name: &'static str,
    /// Verbatim module text.
    pub(crate) text: &'static str,
}

/// Embed one program relative to the pinned 1965 inventory.
macro_rules! mad_program {
    ($name:literal) => {
        MadProgram {
            name: $name,
            text: include_str!(concat!("../programs/1965/", $name)),
        }
    };
}

/// Pinned 1965 ELIZA and SLIP MAD modules.
pub(crate) const MAD_1965: &[MadProgram] = &[
    mad_program!("eliza/change.mad"),
    mad_program!("eliza/docbcd.mad"),
    mad_program!("eliza/eliza.mad"),
    mad_program!("eliza/lprint.mad"),
    mad_program!("eliza/tests.mad"),
    mad_program!("eliza/tprint.mad"),
    mad_program!("slip/core/adas.mad"),
    mad_program!("slip/core/equal.mad"),
    mad_program!("slip/core/erardr.mad"),
    mad_program!("slip/core/iralst.mad"),
    mad_program!("slip/core/itsval.mad"),
    mad_program!("slip/core/lcntr.mad"),
    mad_program!("slip/core/list.mad"),
    mad_program!("slip/core/listmt.mad"),
    mad_program!("slip/core/listrd.mad"),
    mad_program!("slip/core/madatr.mad"),
    mad_program!("slip/core/madin.mad"),
    mad_program!("slip/core/makedl.mad"),
    mad_program!("slip/core/namtst.mad"),
    mad_program!("slip/core/newtop.mad"),
    mad_program!("slip/core/newval.mad"),
    mad_program!("slip/core/noatvl.mad"),
    mad_program!("slip/core/popper.mad"),
    mad_program!("slip/core/put.mad"),
    mad_program!("slip/core/reader.mad"),
    mad_program!("slip/core/remove.mad"),
    mad_program!("slip/core/rvect.mad"),
    mad_program!("slip/core/seqrdr.mad"),
    mad_program!("slip/core/subst.mad"),
    mad_program!("slip/core/top.mad"),
    mad_program!("slip/core/xmino.mad"),
    mad_program!("slip/eliza/assmbl.mad"),
    mad_program!("slip/eliza/cntspc.mad"),
    mad_program!("slip/eliza/conlst.mad"),
    mad_program!("slip/eliza/das.mad"),
    mad_program!("slip/eliza/frbcd.mad"),
    mad_program!("slip/eliza/goody.mad"),
    mad_program!("slip/eliza/listp.mad"),
    mad_program!("slip/eliza/lnkbot.mad"),
    mad_program!("slip/eliza/lpntr.mad"),
    mad_program!("slip/eliza/lspntr.mad"),
    mad_program!("slip/eliza/lsscpy.mad"),
    mad_program!("slip/eliza/lsteql.mad"),
    mad_program!("slip/eliza/nodlst.mad"),
    mad_program!("slip/eliza/partn.mad"),
    mad_program!("slip/eliza/place.mad"),
    mad_program!("slip/eliza/rdrrev.mad"),
    mad_program!("slip/eliza/sequen.mad"),
    mad_program!("slip/eliza/split.mad"),
    mad_program!("slip/eliza/subtop.mad"),
    mad_program!("slip/eliza/txtprt.mad"),
    mad_program!("slip/eliza/xlook.mad"),
    mad_program!("slip/eliza/xmatch.mad"),
    mad_program!("slip/eliza/ymatch.mad"),
    mad_program!("slip/reconstructed/bcdit.mad"),
    mad_program!("slip/reconstructed/inlstl.mad"),
    mad_program!("slip/reconstructed/letter.mad"),
];
