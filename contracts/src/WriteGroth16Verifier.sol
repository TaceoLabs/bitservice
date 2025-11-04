// SPDX-License-Identifier: GPL-3.0
/*
    Copyright 2021 0KIMS association.

    This file is generated with [snarkJS](https://github.com/iden3/snarkjs).

    snarkJS is a free software: you can redistribute it and/or modify it
    under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    snarkJS is distributed in the hope that it will be useful, but WITHOUT
    ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
    or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public
    License for more details.

    You should have received a copy of the GNU General Public License
    along with snarkJS. If not, see <https://www.gnu.org/licenses/>.
*/

pragma solidity >=0.7.0 <0.9.0;

contract Groth16Verifier {
    // Scalar field size
    uint256 constant r    = 21888242871839275222246405745257275088548364400416034343698204186575808495617;
    // Base field size
    uint256 constant q   = 21888242871839275222246405745257275088696311157297823662689037894645226208583;

    // Verification Key data
    uint256 constant alphax  = 4263053894204522186672711677802516615899161868238366406171528269288234330409;
    uint256 constant alphay  = 3750300496301667122784075653927695158870010365700660213344230436097512336693;
    uint256 constant betax1  = 15534935993398879165516102645337112024090009213129419331220227602801070443265;
    uint256 constant betax2  = 347239225313144582750878056409808340028560118983906938534830591335995536336;
    uint256 constant betay1  = 21652484769066758824886337872740377767124613855181550639031158638130366512145;
    uint256 constant betay2  = 14961411945640268149198033773351805943166731769817687936122888828545024873609;
    uint256 constant gammax1 = 2689119435880466389101653884378962295422699469986467706103401854494044326714;
    uint256 constant gammax2 = 16247642080019381518795808835349860371717653597133741855550339798542466722104;
    uint256 constant gammay1 = 21297961835542706434036695040581332254581183105088580691464021905478998604188;
    uint256 constant gammay2 = 15666104728493222273218694750209651890352739527890547937402923626204152570501;
    uint256 constant deltax1 = 21600313273990929211672237017020251083058912930821273895004961268825646557716;
    uint256 constant deltax2 = 2918126483195640777396867318296948864732957028916138285907114466047464896471;
    uint256 constant deltay1 = 11311335639171701431037833901434832522221401506066304656904545278346744260275;
    uint256 constant deltay2 = 16419207587523098285086261538487404506943690142839856425208634742024932018620;

    uint256 constant IC0x = 5024410398789941584175494513259239405981984613055824964346949382859580991920;
    uint256 constant IC0y = 20433054136043438183028491830289747820978394607377216010273235963893030991716;

    uint256 constant IC1x = 15926495051388461408432423242304101210018917352589497980092861839552329334005;
    uint256 constant IC1y = 17313071772207351502988855532228447418392763568063995393033937114913436069045;

    uint256 constant IC2x = 18321754751304183743343923347384119451760208395752960235710107150428646992615;
    uint256 constant IC2y = 13589777541104740076289209205732305459671197641104759890731555285831745797069;

    uint256 constant IC3x = 13240892589122707513741043433146302688293597817343626236390100098120709696962;
    uint256 constant IC3y = 4412731227661244498964744957934219350755293822410299263284531302869376928061;

    uint256 constant IC4x = 622857829398648663737396133986513680761071532626984282491443868789165789497;
    uint256 constant IC4y = 6243435095765637621789866623679192409429126519829348671797683894808114522418;



    // Memory data
    uint16 constant pVk = 0;
    uint16 constant pPairing = 128;

    uint16 constant pLastMem = 896;

    function verifyProof(uint[2] calldata _pA, uint[2][2] calldata _pB, uint[2] calldata _pC, uint[4] calldata _pubSignals) public view returns (bool) {
        assembly {
            function checkField(v) {
                if iszero(lt(v, r)) {
                    mstore(0, 0)
                    return(0, 0x20)
                }
            }

            // G1 function to multiply a G1 value(x,y) to value in an address
            function g1_mulAccC(pR, x, y, s) {
                let success
                let mIn := mload(0x40)
                mstore(mIn, x)
                mstore(add(mIn, 32), y)
                mstore(add(mIn, 64), s)

                success := staticcall(sub(gas(), 2000), 7, mIn, 96, mIn, 64)

                if iszero(success) {
                    mstore(0, 0)
                    return(0, 0x20)
                }

                mstore(add(mIn, 64), mload(pR))
                mstore(add(mIn, 96), mload(add(pR, 32)))

                success := staticcall(sub(gas(), 2000), 6, mIn, 128, pR, 64)

                if iszero(success) {
                    mstore(0, 0)
                    return(0, 0x20)
                }
            }

            function checkPairing(pA, pB, pC, pubSignals, pMem) -> isOk {
                let _pPairing := add(pMem, pPairing)
                let _pVk := add(pMem, pVk)

                mstore(_pVk, IC0x)
                mstore(add(_pVk, 32), IC0y)

                // Compute the linear combination vk_x
                g1_mulAccC(_pVk, IC1x, IC1y, calldataload(add(pubSignals, 0)))
                g1_mulAccC(_pVk, IC2x, IC2y, calldataload(add(pubSignals, 32)))
                g1_mulAccC(_pVk, IC3x, IC3y, calldataload(add(pubSignals, 64)))
                g1_mulAccC(_pVk, IC4x, IC4y, calldataload(add(pubSignals, 96)))


                // -A
                mstore(_pPairing, calldataload(pA))
                mstore(add(_pPairing, 32), mod(sub(q, calldataload(add(pA, 32))), q))

                // B
                mstore(add(_pPairing, 64), calldataload(pB))
                mstore(add(_pPairing, 96), calldataload(add(pB, 32)))
                mstore(add(_pPairing, 128), calldataload(add(pB, 64)))
                mstore(add(_pPairing, 160), calldataload(add(pB, 96)))

                // alpha1
                mstore(add(_pPairing, 192), alphax)
                mstore(add(_pPairing, 224), alphay)

                // beta2
                mstore(add(_pPairing, 256), betax1)
                mstore(add(_pPairing, 288), betax2)
                mstore(add(_pPairing, 320), betay1)
                mstore(add(_pPairing, 352), betay2)

                // vk_x
                mstore(add(_pPairing, 384), mload(add(pMem, pVk)))
                mstore(add(_pPairing, 416), mload(add(pMem, add(pVk, 32))))


                // gamma2
                mstore(add(_pPairing, 448), gammax1)
                mstore(add(_pPairing, 480), gammax2)
                mstore(add(_pPairing, 512), gammay1)
                mstore(add(_pPairing, 544), gammay2)

                // C
                mstore(add(_pPairing, 576), calldataload(pC))
                mstore(add(_pPairing, 608), calldataload(add(pC, 32)))

                // delta2
                mstore(add(_pPairing, 640), deltax1)
                mstore(add(_pPairing, 672), deltax2)
                mstore(add(_pPairing, 704), deltay1)
                mstore(add(_pPairing, 736), deltay2)


                let success := staticcall(sub(gas(), 2000), 8, _pPairing, 768, _pPairing, 0x20)

                isOk := and(success, mload(_pPairing))
            }

            let pMem := mload(0x40)
            mstore(0x40, add(pMem, pLastMem))

            // Validate that all evaluations ∈ F
            checkField(calldataload(add(_pubSignals, 0)))
            checkField(calldataload(add(_pubSignals, 32)))
            checkField(calldataload(add(_pubSignals, 64)))
            checkField(calldataload(add(_pubSignals, 96)))


            // Validate all evaluations
            let isValid := checkPairing(_pA, _pB, _pC, _pubSignals, pMem)

            mstore(0, isValid)
             return(0, 0x20)
         }
     }
 }
